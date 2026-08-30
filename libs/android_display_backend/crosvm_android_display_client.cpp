/*
 * Copyright 2024 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#include <aidl/android/crosvm/BnCrosvmAndroidDisplayService.h>
#include <aidl/android/crosvm/DisplayConfig.h>
#include <android-base/logging.h>
#include <android-base/result.h>
#include <android-base/scopeguard.h>
#include <android/binder_manager.h>
#include <android/binder_process.h>
#include <android/hardware_buffer.h>
#define ATRACE_TAG ATRACE_TAG_GRAPHICS
#include <cutils/trace.h>
#include <system/graphics.h> // for HAL_PIXEL_FORMAT_*
#include <system/window.h>
#include <vndk/window.h>
#define VK_NO_PROTOTYPES
#include <dlfcn.h>
#include <poll.h>
#include <sys/stat.h>
#include <unistd.h>
#include <vulkan/vulkan.h>
#include <vulkan/vulkan_android.h>

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <condition_variable>
#include <cstdlib>
#include <cstring>
#include <iomanip>
#include <iterator>
#include <memory>
#include <mutex>
#include <unordered_map>
#include <vector>

#if defined(__aarch64__)
#include <arm_neon.h>
#endif

#include "media_codec_dl.h"
#include "surface_control_dl.h"

using aidl::android::crosvm::BnCrosvmAndroidDisplayService;
using aidl::android::view::Surface;

using android::base::Error;
using android::base::Result;

namespace {

class ScopedTrace {
public:
    explicit ScopedTrace(const char* name) { ATRACE_BEGIN(name); }
    ~ScopedTrace() { ATRACE_END(); }

    ScopedTrace(const ScopedTrace&) = delete;
    ScopedTrace& operator=(const ScopedTrace&) = delete;
};

bool envFlagEnabled(const char* name, bool defaultValue) {
    const char* value = std::getenv(name);
    if (value == nullptr || *value == '\0') return defaultValue;
    if (std::strcmp(value, "1") == 0 || std::strcmp(value, "true") == 0 ||
        std::strcmp(value, "on") == 0) {
        return true;
    }
    if (std::strcmp(value, "0") == 0 || std::strcmp(value, "false") == 0 ||
        std::strcmp(value, "off") == 0) {
        return false;
    }
    LOG(WARNING) << "Ignoring invalid " << name << " value '" << value << "'";
    return defaultValue;
}

bool isSurfaceUnavailableStatus(int status) {
    return status == -ENODEV || status == -EINVAL;
}

enum class RuntimeFlipFailureStage {
    kNone,
    kDequeue,
    kTargetImport,
    kSubmit,
    kQueue,
};

RuntimeFlipFailureStage configuredRuntimeFlipFailureStage() {
    static const RuntimeFlipFailureStage stage = [] {
        const char* value = std::getenv("CROSVM_ANDROID_DISPLAY_FAIL_STAGE");
        if (value == nullptr || *value == '\0') return RuntimeFlipFailureStage::kNone;
        if (std::strcmp(value, "dequeue") == 0) return RuntimeFlipFailureStage::kDequeue;
        if (std::strcmp(value, "target_import") == 0) {
            return RuntimeFlipFailureStage::kTargetImport;
        }
        if (std::strcmp(value, "submit") == 0) return RuntimeFlipFailureStage::kSubmit;
        if (std::strcmp(value, "queue") == 0) return RuntimeFlipFailureStage::kQueue;
        LOG(WARNING) << "Ignoring invalid CROSVM_ANDROID_DISPLAY_FAIL_STAGE value '" << value
                     << "'";
        return RuntimeFlipFailureStage::kNone;
    }();
    return stage;
}

const char* runtimeFlipFailureStageName(RuntimeFlipFailureStage stage) {
    switch (stage) {
        case RuntimeFlipFailureStage::kDequeue:
            return "dequeue";
        case RuntimeFlipFailureStage::kTargetImport:
            return "target_import";
        case RuntimeFlipFailureStage::kSubmit:
            return "submit";
        case RuntimeFlipFailureStage::kQueue:
            return "queue";
        case RuntimeFlipFailureStage::kNone:
            return "none";
    }
    return "unknown";
}

bool injectRuntimeFlipFailure(RuntimeFlipFailureStage stage) {
    if (configuredRuntimeFlipFailureStage() != stage) return false;
    static std::atomic<bool> injected = false;
    if (injected.exchange(true, std::memory_order_relaxed)) return false;
    LOG(WARNING) << "Injecting one-shot Android display runtime flip failure at stage="
                 << runtimeFlipFailureStageName(stage);
    return true;
}

struct HwModuleMethods {
    int (*open)(const void* module, const char* id, void** device);
};

struct HwModule {
    uint32_t tag;
    uint16_t module_api_version;
    uint16_t hal_api_version;
    const char* id;
    const char* name;
    const char* author;
    HwModuleMethods* methods;
    void* dso;
#ifdef __LP64__
    uint64_t reserved[32 - 7];
#else
    uint32_t reserved[32 - 7];
#endif
};

struct HwDevice {
    uint32_t tag;
    uint32_t version;
    void* module;
#ifdef __LP64__
    uint64_t reserved[12];
#else
    uint32_t reserved[12];
#endif
    int (*close)(void* device);
};

struct HwvulkanDevice {
    HwDevice common;
    PFN_vkEnumerateInstanceExtensionProperties enumerate_instance_extensions;
    PFN_vkCreateInstance create_instance;
    PFN_vkGetInstanceProcAddr get_instance_proc_addr;
};

class VulkanDisplayBridge {
public:
    VulkanDisplayBridge()
          : mAsyncBlitEnabled(envFlagEnabled("CROSVM_ANDROID_DISPLAY_ASYNC_BLIT", true)) {
        mReady = initialize();
    }

    ~VulkanDisplayBridge() {
        if (mDevice && mDeviceWaitIdle) mDeviceWaitIdle(mDevice);
        for (auto& slot : mInFlightSlots) {
            if (slot.completionSemaphore && mDestroySemaphore) {
                mDestroySemaphore(mDevice, slot.completionSemaphore, nullptr);
            }
            if (slot.acquireSemaphore && mDestroySemaphore) {
                mDestroySemaphore(mDevice, slot.acquireSemaphore, nullptr);
            }
            if (slot.fence && mDestroyFence) mDestroyFence(mDevice, slot.fence, nullptr);
        }
        clearTargetCache();
        for (auto& [_, imported] : mImports) destroyImport(imported);
        mImports.clear();
        if (mDevice && mTimestampQueryPool && mDestroyQueryPool) {
            mDestroyQueryPool(mDevice, mTimestampQueryPool, nullptr);
        }
        if (mDevice && mCommandPool && mDestroyCommandPool) {
            mDestroyCommandPool(mDevice, mCommandPool, nullptr);
        }
        if (mDevice && mDestroyDevice) mDestroyDevice(mDevice, nullptr);
        if (mInstance && mDestroyInstance) mDestroyInstance(mInstance, nullptr);
        if (mHal && mHal->common.close) mHal->common.close(mHal);
        if (mLibrary) dlclose(mLibrary);
    }

    bool ready() const { return mReady; }

    int64_t importDmabuf(int fd, uint32_t offset, uint32_t stride, uint64_t modifier,
                         bool linearLayoutVerified, uint32_t width, uint32_t height,
                         uint32_t fourcc) {
        if (!mReady || fd < 0 || !width || !height ||
            static_cast<uint64_t>(stride) < static_cast<uint64_t>(width) * 4) {
            return 0;
        }
        if (!linearLayoutVerified) {
            LOG(WARNING) << "refusing direct image import without verified linear provenance";
            return 0;
        }
        if (modifier == kDrmFormatModInvalid) modifier = kDrmFormatModLinear;
        if (modifier != kDrmFormatModLinear) {
            LOG(WARNING) << "unsupported verified display modifier 0x" << std::hex << modifier;
            return 0;
        }

        VkFormat sourceFormat = vkFormatFromDrmFourcc(fourcc);
        if (sourceFormat == VK_FORMAT_UNDEFINED) {
            LOG(ERROR) << "unsupported display DRM fourcc 0x" << std::hex << fourcc;
            return 0;
        }

        ImportedImage imported{};
        imported.width = width;
        imported.height = height;
        if (!createSourceImage(fd, offset, stride, modifier, sourceFormat, imported)) {
            destroyImport(imported);
            return 0;
        }

        int64_t importId = mNextImportId++;
        mImports.emplace(importId, std::move(imported));
        return importId;
    }

    void release(int64_t importId) {
        auto it = mImports.find(importId);
        if (it == mImports.end()) return;
        if (mDeviceWaitIdle) mDeviceWaitIdle(mDevice);
        for (auto& slot : mInFlightSlots) reclaimSlotAfterDeviceIdle(slot);
        destroyImport(it->second);
        mImports.erase(it);
    }

    // Set when the async path can import an acquire fence as a GPU wait semaphore, letting the
    // caller skip the CPU poll on the dequeued release fence.
    bool canImportAcquireFence() const { return mAsyncBlitEnabled && mAsyncAcquireImportSupported; }

    Result<void> resetTargetsForSurfaceChange() {
        if (mTargetCache.empty()) return {};
        if (!mDeviceWaitIdle) return Error() << "Vulkan device idle wait is unavailable";

        const size_t targetCount = mTargetCache.size();
        VkResult result = mDeviceWaitIdle(mDevice);
        if (result != VK_SUCCESS) {
            return Error() << "Failed to drain display targets for surface change: " << result;
        }
        for (auto& slot : mInFlightSlots) reclaimSlotAfterDeviceIdle(slot);
        clearTargetCache();
        LOG(INFO) << "Android display cleared " << targetCount
                  << " cached targets for surface change";
        return {};
    }

    // acquireFenceFd is the release fence returned by ANativeWindow_dequeueBuffer. When
    // canImportAcquireFence() is true the caller passes it here for GPU-side waiting and blit()
    // takes ownership of it; otherwise the caller must already have waited and pass -1.
    Result<int> blit(int64_t importId, AHardwareBuffer* targetAhb, int acquireFenceFd = -1,
                     bool allowFaultInjection = false) {
        ScopedTrace trace("crosvm_display.blit");
        ATRACE_INT64("crosvm_display.frame_id", static_cast<int64_t>(++mFrameSequence));
        // An early error must preserve the dequeued buffer's release dependency before flip()
        // cancels it with fence -1. Normal submission disables this guard after Vulkan consumes
        // the fd.
        auto acquireFenceGuard = android::base::make_scope_guard([&acquireFenceFd] {
            if (acquireFenceFd < 0) return;
            if (auto ret = pollFence(acquireFenceFd); !ret.ok()) {
                LOG(WARNING) << "Failed to drain display acquire fence on blit error: "
                             << ret.error();
            }
            acquireFenceFd = -1;
        });
        auto it = mImports.find(importId);
        if (it == mImports.end()) return Error() << "Unknown display import " << importId;
        ImportedImage& image = it->second;
        if (mInFlightSlots.empty()) return Error() << "No Vulkan display in-flight slots";

        if (allowFaultInjection &&
            injectRuntimeFlipFailure(RuntimeFlipFailureStage::kTargetImport)) {
            return Error() << "Injected display target import failure";
        }
        auto targetResult = acquireTargetImage(targetAhb, image.width, image.height);
        if (!targetResult.ok()) return targetResult.error();
        TargetImage& target = *targetResult.value();

        const size_t slotIndex = mNextInFlightSlot++ % mInFlightSlots.size();
        InFlightSlot& slot = mInFlightSlots[slotIndex];
        if (auto ret = reclaimSlot(slot); !ret.ok()) return ret.error();

        const bool injectSubmitFailure =
                allowFaultInjection && injectRuntimeFlipFailure(RuntimeFlipFailureStage::kSubmit);
        if (injectSubmitFailure && acquireFenceFd >= 0) {
            acquireFenceGuard.Disable();
            auto waitResult = pollFence(acquireFenceFd);
            acquireFenceFd = -1;
            if (!waitResult.ok()) return waitResult.error();
        }

        const bool recordGpuTimestamps = mTimestampQueryPool && ATRACE_ENABLED();
        const uint32_t timestampQueryBase = static_cast<uint32_t>(slotIndex * 2);
        VkCommandBuffer commandBuffer = slot.commandBuffer;
        {
            ScopedTrace recordTrace("crosvm_display.command_record");
            if (mResetCommandBuffer(commandBuffer, 0) != VK_SUCCESS) {
                evictTarget(target);
                return Error() << "Failed to reset display command buffer";
            }
            VkCommandBufferBeginInfo beginInfo = {
                    .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
                    .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
            };
            if (mBeginCommandBuffer(commandBuffer, &beginInfo) != VK_SUCCESS) {
                evictTarget(target);
                return Error() << "Failed to begin display command buffer";
            }
            if (recordGpuTimestamps) {
                mCmdResetQueryPool(commandBuffer, mTimestampQueryPool, timestampQueryBase, 2);
            }

            VkImageMemoryBarrier acquireBarriers[2] = {
                    {
                            .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                            .srcAccessMask = VK_ACCESS_MEMORY_WRITE_BIT,
                            .dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT,
                            .oldLayout = VK_IMAGE_LAYOUT_GENERAL,
                            .newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                            .srcQueueFamilyIndex = VK_QUEUE_FAMILY_FOREIGN_EXT,
                            .dstQueueFamilyIndex = mQueueFamilyIndex,
                            .image = image.sourceImage,
                            .subresourceRange = colorRange(),
                    },
                    {
                            .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                            .srcAccessMask = 0,
                            .dstAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
                            .oldLayout = VK_IMAGE_LAYOUT_UNDEFINED,
                            .newLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                            .srcQueueFamilyIndex = VK_QUEUE_FAMILY_FOREIGN_EXT,
                            .dstQueueFamilyIndex = mQueueFamilyIndex,
                            .image = target.image,
                            .subresourceRange = colorRange(),
                    },
            };
            mCmdPipelineBarrier(commandBuffer, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
                                VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0, nullptr, 2,
                                acquireBarriers);
            if (recordGpuTimestamps) {
                mCmdWriteTimestamp(commandBuffer, VK_PIPELINE_STAGE_TRANSFER_BIT,
                                   mTimestampQueryPool, timestampQueryBase);
            }

            VkImageBlit region = {
                    .srcSubresource =
                            {
                                    .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
                                    .layerCount = 1,
                            },
                    .srcOffsets = {{0, 0, 0},
                                   {static_cast<int32_t>(image.width),
                                    static_cast<int32_t>(image.height), 1}},
                    .dstSubresource =
                            {
                                    .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
                                    .layerCount = 1,
                            },
                    .dstOffsets = {{0, 0, 0},
                                   {static_cast<int32_t>(image.width),
                                    static_cast<int32_t>(image.height), 1}},
            };
            mCmdBlitImage(commandBuffer, image.sourceImage, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                          target.image, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region,
                          VK_FILTER_NEAREST);
            if (recordGpuTimestamps) {
                mCmdWriteTimestamp(commandBuffer, VK_PIPELINE_STAGE_TRANSFER_BIT,
                                   mTimestampQueryPool, timestampQueryBase + 1);
            }

            VkImageMemoryBarrier releaseBarriers[2] = {
                    {
                            .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                            .srcAccessMask = VK_ACCESS_TRANSFER_READ_BIT,
                            .dstAccessMask = VK_ACCESS_MEMORY_WRITE_BIT,
                            .oldLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                            .newLayout = VK_IMAGE_LAYOUT_GENERAL,
                            .srcQueueFamilyIndex = mQueueFamilyIndex,
                            .dstQueueFamilyIndex = VK_QUEUE_FAMILY_FOREIGN_EXT,
                            .image = image.sourceImage,
                            .subresourceRange = colorRange(),
                    },
                    {
                            .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                            .srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT,
                            .dstAccessMask = VK_ACCESS_MEMORY_READ_BIT,
                            .oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                            .newLayout = VK_IMAGE_LAYOUT_GENERAL,
                            .srcQueueFamilyIndex = mQueueFamilyIndex,
                            .dstQueueFamilyIndex = VK_QUEUE_FAMILY_FOREIGN_EXT,
                            .image = target.image,
                            .subresourceRange = colorRange(),
                    },
            };
            mCmdPipelineBarrier(commandBuffer, VK_PIPELINE_STAGE_TRANSFER_BIT,
                                VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr, 0, nullptr, 2,
                                releaseBarriers);

            if (mEndCommandBuffer(commandBuffer) != VK_SUCCESS) {
                evictTarget(target);
                return Error() << "Failed to end display command buffer";
            }
        }

        VkSemaphore signalSemaphore = VK_NULL_HANDLE;
        if (mAsyncBlitEnabled) {
            VkExportSemaphoreCreateInfo exportInfo = {
                    .sType = VK_STRUCTURE_TYPE_EXPORT_SEMAPHORE_CREATE_INFO,
                    .handleTypes = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_SYNC_FD_BIT,
            };
            VkSemaphoreCreateInfo semaphoreInfo = {
                    .sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,
                    .pNext = &exportInfo,
            };
            if (mCreateSemaphore(mDevice, &semaphoreInfo, nullptr, &slot.completionSemaphore) !=
                VK_SUCCESS) {
                evictTarget(target);
                return Error() << "Failed to create display completion semaphore";
            }
            signalSemaphore = slot.completionSemaphore;
            if (mResetFences(mDevice, 1, &slot.fence) != VK_SUCCESS) {
                evictTarget(target);
                mDestroySemaphore(mDevice, slot.completionSemaphore, nullptr);
                slot.completionSemaphore = VK_NULL_HANDLE;
                return Error() << "Failed to reset display in-flight fence";
            }
        }

        // Import the dequeued release fence as a GPU wait semaphore so the blit waits on target
        // availability without a CPU poll. Only attempted on the async path with SYNC_FD import
        // capability; otherwise the caller has already waited on the fence and passes -1.
        VkSemaphore waitSemaphore = VK_NULL_HANDLE;
        VkPipelineStageFlags waitStage = VK_PIPELINE_STAGE_TRANSFER_BIT;
        if (canImportAcquireFence() && acquireFenceFd >= 0) {
            VkSemaphoreCreateInfo waitSemaphoreInfo = {
                    .sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,
            };
            if (mCreateSemaphore(mDevice, &waitSemaphoreInfo, nullptr, &slot.acquireSemaphore) !=
                VK_SUCCESS) {
                evictTarget(target);
                if (slot.completionSemaphore) {
                    mDestroySemaphore(mDevice, slot.completionSemaphore, nullptr);
                    slot.completionSemaphore = VK_NULL_HANDLE;
                }
                return Error() << "Failed to create display acquire semaphore";
            }
            VkImportSemaphoreFdInfoKHR importInfo = {
                    .sType = VK_STRUCTURE_TYPE_IMPORT_SEMAPHORE_FD_INFO_KHR,
                    .semaphore = slot.acquireSemaphore,
                    .flags = VK_SEMAPHORE_IMPORT_TEMPORARY_BIT,
                    .handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_SYNC_FD_BIT,
                    .fd = acquireFenceFd,
            };
            if (mImportSemaphoreFd(mDevice, &importInfo) != VK_SUCCESS) {
                // On success Vulkan owns the fd; on failure it stays ours. Fall back to a CPU wait
                // rather than dropping the acquire dependency, which would risk tearing.
                mDestroySemaphore(mDevice, slot.acquireSemaphore, nullptr);
                slot.acquireSemaphore = VK_NULL_HANDLE;
                acquireFenceGuard.Disable(); // pollFence consumes (closes) the fd
                if (auto ret = pollFence(acquireFenceFd); !ret.ok()) {
                    evictTarget(target);
                    if (slot.completionSemaphore) {
                        mDestroySemaphore(mDevice, slot.completionSemaphore, nullptr);
                        slot.completionSemaphore = VK_NULL_HANDLE;
                    }
                    return ret.error();
                }
            } else {
                waitSemaphore = slot.acquireSemaphore;
                acquireFenceGuard.Disable(); // vkImportSemaphoreFdKHR consumed the fd
            }
        }

        VkSubmitInfo submitInfo = {
                .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
                .waitSemaphoreCount = waitSemaphore == VK_NULL_HANDLE ? 0u : 1u,
                .pWaitSemaphores = waitSemaphore == VK_NULL_HANDLE ? nullptr : &waitSemaphore,
                .pWaitDstStageMask = waitSemaphore == VK_NULL_HANDLE ? nullptr : &waitStage,
                .commandBufferCount = 1,
                .pCommandBuffers = &commandBuffer,
                .signalSemaphoreCount = signalSemaphore == VK_NULL_HANDLE ? 0u : 1u,
                .pSignalSemaphores = signalSemaphore == VK_NULL_HANDLE ? nullptr : &signalSemaphore,
        };
        VkResult result;
        {
            ScopedTrace submitTrace("crosvm_display.queue_submit");
            result = injectSubmitFailure
                    ? VK_ERROR_UNKNOWN
                    : mQueueSubmit(mQueue, 1, &submitInfo,
                                   mAsyncBlitEnabled ? slot.fence : VK_NULL_HANDLE);
        }
        if (result != VK_SUCCESS) {
            evictTarget(target);
            if (slot.completionSemaphore) {
                mDestroySemaphore(mDevice, slot.completionSemaphore, nullptr);
                slot.completionSemaphore = VK_NULL_HANDLE;
            }
            if (slot.acquireSemaphore) {
                mDestroySemaphore(mDevice, slot.acquireSemaphore, nullptr);
                slot.acquireSemaphore = VK_NULL_HANDLE;
            }
            return Error() << "Failed to submit display blit: " << result;
        }

        if (!mAsyncBlitEnabled) {
            ScopedTrace waitTrace("crosvm_display.queue_wait_idle");
            result = mQueueWaitIdle(mQueue);
            if (result == VK_SUCCESS && recordGpuTimestamps) {
                collectGpuBlitDuration(timestampQueryBase);
            }
            // Target stays cached for reuse; only the blit completion is awaited here.
            if (result != VK_SUCCESS) return Error() << "Failed to wait for display queue";
            return -1;
        }

        slot.inFlight = true;
        slot.timestampPending = recordGpuTimestamps;
        VkSemaphoreGetFdInfoKHR fdInfo = {
                .sType = VK_STRUCTURE_TYPE_SEMAPHORE_GET_FD_INFO_KHR,
                .semaphore = slot.completionSemaphore,
                .handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_SYNC_FD_BIT,
        };
        int completionFd = -1;
        result = mGetSemaphoreFd(mDevice, &fdInfo, &completionFd);
        if (result != VK_SUCCESS || completionFd < 0) {
            LOG(WARNING) << "Failed to export display completion sync_fd; draining slot: "
                         << result;
            if (auto ret = reclaimSlot(slot); !ret.ok()) return ret.error();
            return -1;
        }
        return completionFd;
    }

private:
    struct ImportedImage {
        VkImage sourceImage = VK_NULL_HANDLE;
        VkDeviceMemory sourceMemory = VK_NULL_HANDLE;
        uint32_t width = 0;
        uint32_t height = 0;
    };

    struct TargetImage {
        VkImage image = VK_NULL_HANDLE;
        VkDeviceMemory memory = VK_NULL_HANDLE;
        AHardwareBuffer* ahb = nullptr;
        uint32_t width = 0;
        uint32_t height = 0;
    };

    struct InFlightSlot {
        VkCommandBuffer commandBuffer = VK_NULL_HANDLE;
        VkFence fence = VK_NULL_HANDLE;
        VkSemaphore completionSemaphore = VK_NULL_HANDLE;
        VkSemaphore acquireSemaphore = VK_NULL_HANDLE;
        bool inFlight = false;
        bool timestampPending = false;
    };

    template <typename T>
    T instanceProc(const char* name) {
        return reinterpret_cast<T>(mHal->get_instance_proc_addr(mInstance, name));
    }

    template <typename T>
    T deviceProc(const char* name) {
        return reinterpret_cast<T>(mGetDeviceProcAddr(mDevice, name));
    }

    static VkImageSubresourceRange colorRange() {
        return {
                .aspectMask = VK_IMAGE_ASPECT_COLOR_BIT,
                .baseMipLevel = 0,
                .levelCount = 1,
                .baseArrayLayer = 0,
                .layerCount = 1,
        };
    }

    static constexpr uint32_t fourcc(char a, char b, char c, char d) {
        return static_cast<uint32_t>(a) | (static_cast<uint32_t>(b) << 8) |
                (static_cast<uint32_t>(c) << 16) | (static_cast<uint32_t>(d) << 24);
    }

    static VkFormat vkFormatFromDrmFourcc(uint32_t format) {
        switch (format) {
            case fourcc('X', 'R', '2', '4'):
            case fourcc('A', 'R', '2', '4'):
                return VK_FORMAT_B8G8R8A8_UNORM;
            case fourcc('X', 'B', '2', '4'):
            case fourcc('A', 'B', '2', '4'):
                return VK_FORMAT_R8G8B8A8_UNORM;
            default:
                return VK_FORMAT_UNDEFINED;
        }
    }

    static uint32_t firstMemoryType(uint32_t bits) {
        return bits ? static_cast<uint32_t>(__builtin_ctz(bits)) : UINT32_MAX;
    }

    static constexpr uint64_t kDrmFormatModLinear = 0;
    static constexpr uint64_t kDrmFormatModInvalid = 0x00ffffffffffffffULL;

    void initializeGpuTimestamps() {
        if (!mTimestampValidBits || !mGetPhysicalDeviceProperties || !mCreateQueryPool ||
            !mDestroyQueryPool || !mCmdResetQueryPool || !mCmdWriteTimestamp ||
            !mGetQueryPoolResults) {
            LOG(INFO) << "Android display GPU timestamps are unavailable";
            return;
        }

        VkPhysicalDeviceProperties properties{};
        mGetPhysicalDeviceProperties(mPhysicalDevice, &properties);
        mTimestampPeriodNs = properties.limits.timestampPeriod;
        if (mTimestampPeriodNs <= 0.0f) return;

        VkQueryPoolCreateInfo queryInfo = {
                .sType = VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO,
                .queryType = VK_QUERY_TYPE_TIMESTAMP,
                .queryCount = static_cast<uint32_t>(mInFlightSlots.size() * 2),
        };
        if (mCreateQueryPool(mDevice, &queryInfo, nullptr, &mTimestampQueryPool) != VK_SUCCESS) {
            mTimestampQueryPool = VK_NULL_HANDLE;
            LOG(WARNING) << "Failed to create Android display GPU timestamp query pool";
            return;
        }
        LOG(INFO) << "Android display GPU timestamps enabled: period=" << mTimestampPeriodNs
                  << " ns, validBits=" << mTimestampValidBits;
    }

    void collectGpuBlitDuration(uint32_t queryBase) {
        if (!mTimestampQueryPool) return;
        uint64_t timestamps[2]{};
        VkResult result =
                mGetQueryPoolResults(mDevice, mTimestampQueryPool, queryBase, 2, sizeof(timestamps),
                                     timestamps, sizeof(uint64_t), VK_QUERY_RESULT_64_BIT);
        if (result != VK_SUCCESS) {
            if (!mTimestampReadFailureLogged) {
                mTimestampReadFailureLogged = true;
                LOG(WARNING) << "Failed to read Android display GPU timestamps: " << result;
            }
            return;
        }

        uint64_t elapsedTicks = timestamps[1] - timestamps[0];
        if (mTimestampValidBits < 64) {
            const uint64_t mask = (uint64_t{1} << mTimestampValidBits) - 1;
            elapsedTicks &= mask;
        }
        const uint64_t durationNs =
                static_cast<uint64_t>(static_cast<double>(elapsedTicks) * mTimestampPeriodNs);
        ATRACE_INT64("crosvm_display.gpu_blit_ns", static_cast<int64_t>(durationNs));
    }

    Result<void> reclaimSlot(InFlightSlot& slot) {
        if (!slot.inFlight) return {};
        VkResult result;
        {
            ScopedTrace waitTrace("crosvm_display.in_flight_wait");
            result = mWaitForFences(mDevice, 1, &slot.fence, VK_TRUE, UINT64_MAX);
        }
        if (result != VK_SUCCESS) {
            return Error() << "Failed to wait for display in-flight slot: " << result;
        }
        slot.inFlight = false;
        if (slot.timestampPending) {
            const size_t slotIndex = static_cast<size_t>(&slot - mInFlightSlots.data());
            collectGpuBlitDuration(static_cast<uint32_t>(slotIndex * 2));
            slot.timestampPending = false;
        }
        // Target images live in mTargetCache and outlive individual slots; do not destroy here.
        if (slot.completionSemaphore) {
            mDestroySemaphore(mDevice, slot.completionSemaphore, nullptr);
            slot.completionSemaphore = VK_NULL_HANDLE;
        }
        if (slot.acquireSemaphore) {
            mDestroySemaphore(mDevice, slot.acquireSemaphore, nullptr);
            slot.acquireSemaphore = VK_NULL_HANDLE;
        }
        return {};
    }

    void reclaimSlotAfterDeviceIdle(InFlightSlot& slot) {
        if (!slot.inFlight) return;
        slot.inFlight = false;
        if (slot.timestampPending) {
            const size_t slotIndex = static_cast<size_t>(&slot - mInFlightSlots.data());
            collectGpuBlitDuration(static_cast<uint32_t>(slotIndex * 2));
            slot.timestampPending = false;
        }
        // Target images live in mTargetCache and outlive individual slots; do not destroy here.
        if (slot.completionSemaphore) {
            mDestroySemaphore(mDevice, slot.completionSemaphore, nullptr);
            slot.completionSemaphore = VK_NULL_HANDLE;
        }
        if (slot.acquireSemaphore) {
            mDestroySemaphore(mDevice, slot.acquireSemaphore, nullptr);
            slot.acquireSemaphore = VK_NULL_HANDLE;
        }
    }

    bool modifierSupportsBlitSource(VkFormat format, uint64_t modifier) const {
        VkDrmFormatModifierPropertiesListEXT modifierList = {
                .sType = VK_STRUCTURE_TYPE_DRM_FORMAT_MODIFIER_PROPERTIES_LIST_EXT,
        };
        VkFormatProperties2 properties = {
                .sType = VK_STRUCTURE_TYPE_FORMAT_PROPERTIES_2,
                .pNext = &modifierList,
        };
        mGetPhysicalDeviceFormatProperties2(mPhysicalDevice, format, &properties);
        if (modifierList.drmFormatModifierCount == 0) return false;

        std::vector<VkDrmFormatModifierPropertiesEXT> modifiers(
                modifierList.drmFormatModifierCount);
        modifierList.pDrmFormatModifierProperties = modifiers.data();
        mGetPhysicalDeviceFormatProperties2(mPhysicalDevice, format, &properties);
        constexpr VkFormatFeatureFlags required =
                VK_FORMAT_FEATURE_TRANSFER_SRC_BIT | VK_FORMAT_FEATURE_BLIT_SRC_BIT;
        for (const auto& candidate : modifiers) {
            if (candidate.drmFormatModifier == modifier &&
                candidate.drmFormatModifierPlaneCount == 1 &&
                (candidate.drmFormatModifierTilingFeatures & required) == required) {
                return true;
            }
        }
        return false;
    }

    bool externalImageImportSupported(VkFormat format, VkImageTiling tiling,
                                      VkImageUsageFlags usage,
                                      VkExternalMemoryHandleTypeFlagBits handleType,
                                      const void* externalInfoNext = nullptr) const {
        VkPhysicalDeviceExternalImageFormatInfo externalInfo = {
                .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_IMAGE_FORMAT_INFO,
                .pNext = externalInfoNext,
                .handleType = handleType,
        };
        VkPhysicalDeviceImageFormatInfo2 imageInfo = {
                .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_IMAGE_FORMAT_INFO_2,
                .pNext = &externalInfo,
                .format = format,
                .type = VK_IMAGE_TYPE_2D,
                .tiling = tiling,
                .usage = usage,
        };
        VkExternalImageFormatProperties externalProperties = {
                .sType = VK_STRUCTURE_TYPE_EXTERNAL_IMAGE_FORMAT_PROPERTIES,
        };
        VkImageFormatProperties2 imageProperties = {
                .sType = VK_STRUCTURE_TYPE_IMAGE_FORMAT_PROPERTIES_2,
                .pNext = &externalProperties,
        };
        if (mGetPhysicalDeviceImageFormatProperties2(mPhysicalDevice, &imageInfo,
                                                     &imageProperties) != VK_SUCCESS) {
            return false;
        }
        const auto& memoryProperties = externalProperties.externalMemoryProperties;
        return (memoryProperties.externalMemoryFeatures &
                VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT) != 0 &&
                (memoryProperties.compatibleHandleTypes & handleType) != 0;
    }

    bool asyncSemaphoreSupported(
            PFN_vkEnumerateDeviceExtensionProperties enumerateDeviceExtensionProperties) {
        if (!enumerateDeviceExtensionProperties || !mGetPhysicalDeviceExternalSemaphoreProperties) {
            return false;
        }
        uint32_t extensionCount = 0;
        if (enumerateDeviceExtensionProperties(mPhysicalDevice, nullptr, &extensionCount,
                                               nullptr) != VK_SUCCESS) {
            return false;
        }
        std::vector<VkExtensionProperties> extensions(extensionCount);
        if (enumerateDeviceExtensionProperties(mPhysicalDevice, nullptr, &extensionCount,
                                               extensions.data()) != VK_SUCCESS) {
            return false;
        }
        const auto hasExtension = [&extensions](const char* name) {
            return std::any_of(extensions.begin(), extensions.end(), [name](const auto& extension) {
                return std::strcmp(extension.extensionName, name) == 0;
            });
        };
        if (!hasExtension(VK_KHR_EXTERNAL_SEMAPHORE_EXTENSION_NAME) ||
            !hasExtension(VK_KHR_EXTERNAL_SEMAPHORE_FD_EXTENSION_NAME)) {
            return false;
        }

        VkPhysicalDeviceExternalSemaphoreInfo semaphoreInfo = {
                .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_SEMAPHORE_INFO,
                .handleType = VK_EXTERNAL_SEMAPHORE_HANDLE_TYPE_SYNC_FD_BIT,
        };
        VkExternalSemaphoreProperties semaphoreProperties = {
                .sType = VK_STRUCTURE_TYPE_EXTERNAL_SEMAPHORE_PROPERTIES,
        };
        mGetPhysicalDeviceExternalSemaphoreProperties(mPhysicalDevice, &semaphoreInfo,
                                                      &semaphoreProperties);
        // EXPORTABLE is required for the completion semaphore. IMPORTABLE is a separate capability
        // needed only for acquire-fence import; record it so the caller can gate that path.
        mSyncFdImportable = (semaphoreProperties.externalSemaphoreFeatures &
                             VK_EXTERNAL_SEMAPHORE_FEATURE_IMPORTABLE_BIT) != 0;
        return (semaphoreProperties.externalSemaphoreFeatures &
                VK_EXTERNAL_SEMAPHORE_FEATURE_EXPORTABLE_BIT) != 0;
    }

    bool initialize() {
        // The blit driver is any hwvulkan HAL that exposes the required extensions -- turnip, the
        // SoC's vendor driver, PanVK, ... -- so this is not turnip-specific. The chooser (the app's
        // GpuBlitProvider) names one via CROSVM_DISPLAY_VULKAN_LIBRARY; CROSVM_TURNIP_LIBRARY is the
        // former name, still honoured. There is deliberately NO hardcoded fallback: dlopen'ing a
        // fixed world-writable path (e.g. /data/local/tmp) would silently run untrusted native code,
        // so the driver must be named explicitly by whoever launched crosvm. With none named we do
        // not load anything and the caller drops to the CPU copy.
        const char* configuredPath = std::getenv("CROSVM_DISPLAY_VULKAN_LIBRARY");
        if (!configuredPath || !*configuredPath)
            configuredPath = std::getenv("CROSVM_TURNIP_LIBRARY");
        if (!configuredPath || !*configuredPath) {
            LOG(INFO) << "no display Vulkan blit driver configured; using CPU copy";
            return false;
        }
        mLibrary = dlopen(configuredPath, RTLD_NOW | RTLD_LOCAL);
        if (!mLibrary) {
            LOG(ERROR) << "failed to load display Vulkan blit driver from " << configuredPath << ": "
                       << dlerror();
            return false;
        }
        LOG(INFO) << "Android display loading Vulkan blit driver from " << configuredPath;

        auto* module = static_cast<HwModule*>(dlsym(mLibrary, "HMI"));
        if (!module || !module->methods || !module->methods->open ||
            module->methods->open(module, "vk0", reinterpret_cast<void**>(&mHal)) != 0 || !mHal ||
            !mHal->create_instance || !mHal->get_instance_proc_addr) {
            LOG(ERROR) << "invalid display Vulkan hwvulkan HMI";
            return false;
        }

        VkApplicationInfo appInfo = {
                .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
                .pApplicationName = "crosvm_android_display",
                .applicationVersion = 1,
                .pEngineName = "crosvm",
                .engineVersion = 1,
                .apiVersion = VK_API_VERSION_1_1,
        };
        VkInstanceCreateInfo instanceInfo = {
                .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                .pApplicationInfo = &appInfo,
        };
        if (mHal->create_instance(&instanceInfo, nullptr, &mInstance) != VK_SUCCESS) return false;

        auto enumeratePhysicalDevices =
                instanceProc<PFN_vkEnumeratePhysicalDevices>("vkEnumeratePhysicalDevices");
        auto enumerateDeviceExtensionProperties =
                instanceProc<PFN_vkEnumerateDeviceExtensionProperties>(
                        "vkEnumerateDeviceExtensionProperties");
        auto getQueueFamilyProperties = instanceProc<PFN_vkGetPhysicalDeviceQueueFamilyProperties>(
                "vkGetPhysicalDeviceQueueFamilyProperties");
        auto createDevice = instanceProc<PFN_vkCreateDevice>("vkCreateDevice");
        mGetPhysicalDeviceProperties =
                instanceProc<PFN_vkGetPhysicalDeviceProperties>("vkGetPhysicalDeviceProperties");
        mGetPhysicalDeviceFormatProperties2 =
                instanceProc<PFN_vkGetPhysicalDeviceFormatProperties2>(
                        "vkGetPhysicalDeviceFormatProperties2");
        mGetPhysicalDeviceImageFormatProperties2 =
                instanceProc<PFN_vkGetPhysicalDeviceImageFormatProperties2>(
                        "vkGetPhysicalDeviceImageFormatProperties2");
        mGetPhysicalDeviceExternalSemaphoreProperties =
                instanceProc<PFN_vkGetPhysicalDeviceExternalSemaphoreProperties>(
                        "vkGetPhysicalDeviceExternalSemaphoreProperties");
        mGetDeviceProcAddr = instanceProc<PFN_vkGetDeviceProcAddr>("vkGetDeviceProcAddr");
        mDestroyInstance = instanceProc<PFN_vkDestroyInstance>("vkDestroyInstance");
        if (!enumeratePhysicalDevices || !enumerateDeviceExtensionProperties ||
            !getQueueFamilyProperties || !createDevice || !mGetPhysicalDeviceFormatProperties2 ||
            !mGetPhysicalDeviceImageFormatProperties2 || !mGetDeviceProcAddr || !mDestroyInstance) {
            return false;
        }

        uint32_t physicalDeviceCount = 0;
        if (enumeratePhysicalDevices(mInstance, &physicalDeviceCount, nullptr) != VK_SUCCESS ||
            !physicalDeviceCount) {
            return false;
        }
        std::vector<VkPhysicalDevice> physicalDevices(physicalDeviceCount);
        if (enumeratePhysicalDevices(mInstance, &physicalDeviceCount, physicalDevices.data()) !=
            VK_SUCCESS) {
            return false;
        }
        mPhysicalDevice = physicalDevices[0];

        if (mAsyncBlitEnabled && !asyncSemaphoreSupported(enumerateDeviceExtensionProperties)) {
            LOG(WARNING) << "Android display async blit SYNC_FD capability is unavailable; "
                            "keeping synchronous queue waits";
            mAsyncBlitEnabled = false;
        }

        uint32_t queueFamilyCount = 0;
        getQueueFamilyProperties(mPhysicalDevice, &queueFamilyCount, nullptr);
        if (!queueFamilyCount) return false;
        std::vector<VkQueueFamilyProperties> queueFamilies(queueFamilyCount);
        getQueueFamilyProperties(mPhysicalDevice, &queueFamilyCount, queueFamilies.data());
        for (uint32_t i = 0; i < queueFamilyCount; ++i) {
            if (queueFamilies[i].queueFlags & (VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_TRANSFER_BIT)) {
                mQueueFamilyIndex = i;
                mTimestampValidBits = queueFamilies[i].timestampValidBits;
                break;
            }
        }

        float priority = 1.0f;
        VkDeviceQueueCreateInfo queueInfo = {
                .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
                .queueFamilyIndex = mQueueFamilyIndex,
                .queueCount = 1,
                .pQueuePriorities = &priority,
        };
        std::vector<const char*> extensions = {
                VK_EXT_EXTERNAL_MEMORY_DMA_BUF_EXTENSION_NAME,
                VK_EXT_IMAGE_DRM_FORMAT_MODIFIER_EXTENSION_NAME,
                VK_ANDROID_EXTERNAL_MEMORY_ANDROID_HARDWARE_BUFFER_EXTENSION_NAME,
                VK_KHR_EXTERNAL_MEMORY_FD_EXTENSION_NAME,
                VK_EXT_QUEUE_FAMILY_FOREIGN_EXTENSION_NAME,
        };
        if (mAsyncBlitEnabled) {
            extensions.push_back(VK_KHR_EXTERNAL_SEMAPHORE_EXTENSION_NAME);
            extensions.push_back(VK_KHR_EXTERNAL_SEMAPHORE_FD_EXTENSION_NAME);
        }
        VkDeviceCreateInfo deviceInfo = {
                .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
                .queueCreateInfoCount = 1,
                .pQueueCreateInfos = &queueInfo,
                .enabledExtensionCount = static_cast<uint32_t>(extensions.size()),
                .ppEnabledExtensionNames = extensions.data(),
        };
        if (createDevice(mPhysicalDevice, &deviceInfo, nullptr, &mDevice) != VK_SUCCESS)
            return false;

#define LOAD_DEVICE_PROC(member, name) member = deviceProc<decltype(member)>(name)
        LOAD_DEVICE_PROC(mDestroyDevice, "vkDestroyDevice");
        LOAD_DEVICE_PROC(mDeviceWaitIdle, "vkDeviceWaitIdle");
        LOAD_DEVICE_PROC(mGetDeviceQueue, "vkGetDeviceQueue");
        LOAD_DEVICE_PROC(mCreateCommandPool, "vkCreateCommandPool");
        LOAD_DEVICE_PROC(mDestroyCommandPool, "vkDestroyCommandPool");
        LOAD_DEVICE_PROC(mAllocateCommandBuffers, "vkAllocateCommandBuffers");
        LOAD_DEVICE_PROC(mResetCommandBuffer, "vkResetCommandBuffer");
        LOAD_DEVICE_PROC(mBeginCommandBuffer, "vkBeginCommandBuffer");
        LOAD_DEVICE_PROC(mEndCommandBuffer, "vkEndCommandBuffer");
        LOAD_DEVICE_PROC(mQueueSubmit, "vkQueueSubmit");
        LOAD_DEVICE_PROC(mQueueWaitIdle, "vkQueueWaitIdle");
        LOAD_DEVICE_PROC(mCreateFence, "vkCreateFence");
        LOAD_DEVICE_PROC(mDestroyFence, "vkDestroyFence");
        LOAD_DEVICE_PROC(mWaitForFences, "vkWaitForFences");
        LOAD_DEVICE_PROC(mResetFences, "vkResetFences");
        LOAD_DEVICE_PROC(mCreateSemaphore, "vkCreateSemaphore");
        LOAD_DEVICE_PROC(mDestroySemaphore, "vkDestroySemaphore");
        LOAD_DEVICE_PROC(mGetSemaphoreFd, "vkGetSemaphoreFdKHR");
        LOAD_DEVICE_PROC(mImportSemaphoreFd, "vkImportSemaphoreFdKHR");
        LOAD_DEVICE_PROC(mCmdPipelineBarrier, "vkCmdPipelineBarrier");
        LOAD_DEVICE_PROC(mCmdBlitImage, "vkCmdBlitImage");
        LOAD_DEVICE_PROC(mCreateQueryPool, "vkCreateQueryPool");
        LOAD_DEVICE_PROC(mDestroyQueryPool, "vkDestroyQueryPool");
        LOAD_DEVICE_PROC(mCmdResetQueryPool, "vkCmdResetQueryPool");
        LOAD_DEVICE_PROC(mCmdWriteTimestamp, "vkCmdWriteTimestamp");
        LOAD_DEVICE_PROC(mGetQueryPoolResults, "vkGetQueryPoolResults");
        LOAD_DEVICE_PROC(mCreateImage, "vkCreateImage");
        LOAD_DEVICE_PROC(mDestroyImage, "vkDestroyImage");
        LOAD_DEVICE_PROC(mGetImageMemoryRequirements2, "vkGetImageMemoryRequirements2");
        LOAD_DEVICE_PROC(mAllocateMemory, "vkAllocateMemory");
        LOAD_DEVICE_PROC(mFreeMemory, "vkFreeMemory");
        LOAD_DEVICE_PROC(mBindImageMemory, "vkBindImageMemory");
        LOAD_DEVICE_PROC(mGetMemoryFdProperties, "vkGetMemoryFdPropertiesKHR");
        LOAD_DEVICE_PROC(mGetAhbProperties, "vkGetAndroidHardwareBufferPropertiesANDROID");
#undef LOAD_DEVICE_PROC

        if (!mDestroyDevice || !mDeviceWaitIdle || !mGetDeviceQueue || !mCreateCommandPool ||
            !mDestroyCommandPool || !mAllocateCommandBuffers || !mResetCommandBuffer ||
            !mBeginCommandBuffer || !mEndCommandBuffer || !mQueueSubmit || !mQueueWaitIdle ||
            !mCmdPipelineBarrier || !mCmdBlitImage || !mCreateImage || !mDestroyImage ||
            !mGetImageMemoryRequirements2 || !mAllocateMemory || !mFreeMemory ||
            !mBindImageMemory || !mGetMemoryFdProperties || !mGetAhbProperties) {
            return false;
        }
        if (mAsyncBlitEnabled &&
            (!mCreateFence || !mDestroyFence || !mWaitForFences || !mResetFences ||
             !mCreateSemaphore || !mDestroySemaphore || !mGetSemaphoreFd)) {
            LOG(WARNING) << "Android display async blit Vulkan entry points are unavailable; "
                            "keeping synchronous queue waits";
            mAsyncBlitEnabled = false;
        }
        // Acquire-fence import is an independent capability on top of async blit: it needs the
        // import entry point and SYNC_FD IMPORTABLE support. Without it the async path still runs
        // but the dequeued release fence is waited on the CPU instead of the GPU.
        mAsyncAcquireImportSupported =
                mAsyncBlitEnabled && mImportSemaphoreFd != nullptr && mSyncFdImportable;
        if (mAsyncBlitEnabled) {
            LOG(INFO) << "Android display acquire-fence semaphore import "
                      << (mAsyncAcquireImportSupported ? "enabled (GPU wait)"
                                                       : "unavailable (CPU poll fallback)");
        }

        mGetDeviceQueue(mDevice, mQueueFamilyIndex, 0, &mQueue);
        VkCommandPoolCreateInfo poolInfo = {
                .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
                .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
                .queueFamilyIndex = mQueueFamilyIndex,
        };
        if (mCreateCommandPool(mDevice, &poolInfo, nullptr, &mCommandPool) != VK_SUCCESS) {
            return false;
        }
        VkCommandBufferAllocateInfo commandInfo = {
                .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
                .commandPool = mCommandPool,
                .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
                .commandBufferCount = mAsyncBlitEnabled ? kAsyncInFlightSlotCount : 1,
        };
        mInFlightSlots.resize(commandInfo.commandBufferCount);
        std::vector<VkCommandBuffer> commandBuffers(commandInfo.commandBufferCount);
        if (mAllocateCommandBuffers(mDevice, &commandInfo, commandBuffers.data()) != VK_SUCCESS) {
            return false;
        }
        for (size_t i = 0; i < mInFlightSlots.size(); ++i) {
            mInFlightSlots[i].commandBuffer = commandBuffers[i];
            if (!mAsyncBlitEnabled) continue;
            VkFenceCreateInfo fenceInfo = {
                    .sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO,
                    .flags = VK_FENCE_CREATE_SIGNALED_BIT,
            };
            if (mCreateFence(mDevice, &fenceInfo, nullptr, &mInFlightSlots[i].fence) !=
                VK_SUCCESS) {
                return false;
            }
        }
        LOG(INFO) << "Android display async blit " << (mAsyncBlitEnabled ? "enabled" : "disabled")
                  << ", in-flight slots=" << mInFlightSlots.size();
        initializeGpuTimestamps();
        return true;
    }

    bool createSourceImage(int fd, uint32_t offset, uint32_t stride, uint64_t modifier,
                           VkFormat format, ImportedImage& imported) {
        if (!modifierSupportsBlitSource(format, modifier)) {
            LOG(ERROR) << "LINEAR display source does not support transfer/blit source usage";
            return false;
        }
        VkPhysicalDeviceImageDrmFormatModifierInfoEXT modifierQuery = {
                .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_IMAGE_DRM_FORMAT_MODIFIER_INFO_EXT,
                .drmFormatModifier = modifier,
                .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
        };
        if (!externalImageImportSupported(format, VK_IMAGE_TILING_DRM_FORMAT_MODIFIER_EXT,
                                          VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
                                          VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
                                          &modifierQuery)) {
            LOG(ERROR) << "LINEAR DMA-BUF image import is unsupported for source format " << format;
            return false;
        }

        struct stat fdStat{};
        if (fstat(fd, &fdStat) != 0 || fdStat.st_size <= 0) {
            PLOG(ERROR) << "failed to determine display DMA-BUF size";
            return false;
        }
        const VkDeviceSize allocationSize = static_cast<VkDeviceSize>(fdStat.st_size);
        const VkDeviceSize imageSize = static_cast<VkDeviceSize>(stride) * imported.height;
        const VkDeviceSize imageEnd = static_cast<VkDeviceSize>(offset) + imageSize;
        if (imageSize / stride != imported.height || imageEnd < imageSize ||
            imageEnd > allocationSize) {
            LOG(ERROR) << "display DMA-BUF layout exceeds allocation: offset=" << offset
                       << " stride=" << stride << " height=" << imported.height
                       << " allocation=" << allocationSize;
            return false;
        }

        VkSubresourceLayout planeLayout = {
                .offset = offset,
                .size = 0,
                .rowPitch = stride,
                .arrayPitch = 0,
                .depthPitch = 0,
        };
        VkExternalMemoryImageCreateInfo externalInfo = {
                .sType = VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                .handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
        };
        VkImageDrmFormatModifierExplicitCreateInfoEXT modifierInfo = {
                .sType = VK_STRUCTURE_TYPE_IMAGE_DRM_FORMAT_MODIFIER_EXPLICIT_CREATE_INFO_EXT,
                .pNext = &externalInfo,
                .drmFormatModifier = modifier,
                .drmFormatModifierPlaneCount = 1,
                .pPlaneLayouts = &planeLayout,
        };
        // Adreno's 2D blitter reads the LINEAR source through the texture pipe, which fetches whole
        // rows at the image's PITCH. When the guest stride is padded (a width that is not 64px-
        // aligned -- e.g. 1400 -> stride 5632 = 1408px, 8px of row padding) turnip's a7xx A2D
        // texture fetch on a padded-pitch LINEAR source wedges the GPU, and because that blit shares
        // the Adreno with gfxstream's render thread the whole VM hangs. Describe the source image at
        // its natural unpadded width (stride/bpp) so the texture's WIDTH equals its PITCH/bpp; the
        // blit below still samples only the real [0,width) sub-rectangle, so this is a no-op for
        // unpadded widths (stride == width*bpp) and only widens the declared image for padded ones.
        const uint32_t kBytesPerPixel = 4; // every fourcc vkFormatFromDrmFourcc accepts is 32-bpp
        uint32_t sourceImageWidth = imported.width;
        if ((stride % kBytesPerPixel) == 0 && stride / kBytesPerPixel > sourceImageWidth) {
            sourceImageWidth = stride / kBytesPerPixel;
        }
        VkImageCreateInfo imageInfo = {
                .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                .pNext = &modifierInfo,
                .imageType = VK_IMAGE_TYPE_2D,
                .format = format,
                .extent = {sourceImageWidth, imported.height, 1},
                .mipLevels = 1,
                .arrayLayers = 1,
                .samples = VK_SAMPLE_COUNT_1_BIT,
                .tiling = VK_IMAGE_TILING_DRM_FORMAT_MODIFIER_EXT,
                .usage = VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
                .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
                .initialLayout = VK_IMAGE_LAYOUT_UNDEFINED,
        };
        if (mCreateImage(mDevice, &imageInfo, nullptr, &imported.sourceImage) != VK_SUCCESS) {
            LOG(ERROR) << "failed to create explicit LINEAR display source image";
            return false;
        }

        VkMemoryDedicatedRequirements dedicatedRequirements = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_DEDICATED_REQUIREMENTS,
        };
        VkMemoryRequirements2 requirements = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_REQUIREMENTS_2,
                .pNext = &dedicatedRequirements,
        };
        VkImageMemoryRequirementsInfo2 requirementsInfo = {
                .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_REQUIREMENTS_INFO_2,
                .image = imported.sourceImage,
        };
        mGetImageMemoryRequirements2(mDevice, &requirementsInfo, &requirements);
        if (requirements.memoryRequirements.size > allocationSize) {
            LOG(ERROR) << "display source image requirements exceed DMA-BUF allocation: required="
                       << requirements.memoryRequirements.size << " allocation=" << allocationSize;
            return false;
        }

        int importFd = dup(fd);
        if (importFd < 0) return false;
        VkMemoryFdPropertiesKHR fdProperties = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_FD_PROPERTIES_KHR,
        };
        VkResult result =
                mGetMemoryFdProperties(mDevice, VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
                                       importFd, &fdProperties);
        uint32_t memoryType = firstMemoryType(requirements.memoryRequirements.memoryTypeBits &
                                              fdProperties.memoryTypeBits);
        if (result != VK_SUCCESS || memoryType == UINT32_MAX) {
            close(importFd);
            return false;
        }
        VkMemoryDedicatedAllocateInfo dedicatedInfo = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO,
                .image = imported.sourceImage,
        };
        VkImportMemoryFdInfoKHR importInfo = {
                .sType = VK_STRUCTURE_TYPE_IMPORT_MEMORY_FD_INFO_KHR,
                .pNext = &dedicatedInfo,
                .handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF_BIT_EXT,
                .fd = importFd,
        };
        VkMemoryAllocateInfo allocateInfo = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                .pNext = &importInfo,
                .allocationSize = allocationSize,
                .memoryTypeIndex = memoryType,
        };
        result = mAllocateMemory(mDevice, &allocateInfo, nullptr, &imported.sourceMemory);
        if (result != VK_SUCCESS) {
            close(importFd);
            return false;
        }
        return mBindImageMemory(mDevice, imported.sourceImage, imported.sourceMemory, 0) ==
                VK_SUCCESS;
    }

    bool createTargetImage(AHardwareBuffer* ahb, uint32_t width, uint32_t height,
                           TargetImage& target) {
        if (!ahb) return false;
        AHardwareBuffer_Desc desc{};
        AHardwareBuffer_describe(ahb, &desc);
        if (desc.width != width || desc.height != height) {
            LOG(ERROR) << "display target size mismatch: got " << desc.width << "x" << desc.height
                       << ", expected " << width << "x" << height;
            return false;
        }
        VkAndroidHardwareBufferFormatPropertiesANDROID formatProperties = {
                .sType = VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_FORMAT_PROPERTIES_ANDROID,
        };
        VkAndroidHardwareBufferPropertiesANDROID properties = {
                .sType = VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_PROPERTIES_ANDROID,
                .pNext = &formatProperties,
        };
        VkResult ahbResult;
        {
            ScopedTrace propertiesTrace("crosvm_display.ahb_properties");
            ahbResult = mGetAhbProperties(mDevice, ahb, &properties);
        }
        if (ahbResult != VK_SUCCESS || formatProperties.format == VK_FORMAT_UNDEFINED) {
            return false;
        }
        constexpr VkFormatFeatureFlags requiredTargetFeatures =
                VK_FORMAT_FEATURE_TRANSFER_DST_BIT | VK_FORMAT_FEATURE_BLIT_DST_BIT;
        bool targetImportSupported = false;
        {
            ScopedTrace capabilityTrace("crosvm_display.target_capability_query");
            targetImportSupported = externalImageImportSupported(
                    formatProperties.format, VK_IMAGE_TILING_OPTIMAL,
                    VK_IMAGE_USAGE_TRANSFER_DST_BIT,
                    VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID);
        }
        if ((formatProperties.formatFeatures & requiredTargetFeatures) != requiredTargetFeatures ||
            !targetImportSupported) {
            LOG(ERROR) << "AHB target does not support transfer/blit destination import";
            return false;
        }

        VkExternalMemoryImageCreateInfo externalInfo = {
                .sType = VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
                .handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID,
        };
        VkImageCreateInfo imageInfo = {
                .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                .pNext = &externalInfo,
                .imageType = VK_IMAGE_TYPE_2D,
                .format = formatProperties.format,
                .extent = {width, height, 1},
                .mipLevels = 1,
                .arrayLayers = 1,
                .samples = VK_SAMPLE_COUNT_1_BIT,
                .tiling = VK_IMAGE_TILING_OPTIMAL,
                .usage = VK_IMAGE_USAGE_TRANSFER_DST_BIT,
                .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
                .initialLayout = VK_IMAGE_LAYOUT_UNDEFINED,
        };
        {
            ScopedTrace imageTrace("crosvm_display.target_image_create");
            if (mCreateImage(mDevice, &imageInfo, nullptr, &target.image) != VK_SUCCESS) {
                return false;
            }
        }

        // The AHB external-memory rules require allocationSize and memoryTypeBits from
        // vkGetAndroidHardwareBufferPropertiesANDROID. Querying image memory requirements before
        // binding imported AHB memory is invalid.
        uint32_t memoryType = firstMemoryType(properties.memoryTypeBits);
        if (memoryType == UINT32_MAX) return false;
        VkMemoryDedicatedAllocateInfo dedicatedInfo = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO,
                .image = target.image,
        };
        VkImportAndroidHardwareBufferInfoANDROID importInfo = {
                .sType = VK_STRUCTURE_TYPE_IMPORT_ANDROID_HARDWARE_BUFFER_INFO_ANDROID,
                .pNext = &dedicatedInfo,
                .buffer = ahb,
        };
        VkMemoryAllocateInfo allocateInfo = {
                .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                .pNext = &importInfo,
                .allocationSize = properties.allocationSize,
                .memoryTypeIndex = memoryType,
        };
        {
            ScopedTrace allocateTrace("crosvm_display.target_memory_allocate");
            if (mAllocateMemory(mDevice, &allocateInfo, nullptr, &target.memory) != VK_SUCCESS) {
                return false;
            }
        }
        {
            ScopedTrace bindTrace("crosvm_display.target_memory_bind");
            return mBindImageMemory(mDevice, target.image, target.memory, 0) == VK_SUCCESS;
        }
    }

    void destroyTarget(TargetImage& target) {
        ScopedTrace trace("crosvm_display.target_destroy");
        if (target.image && mDestroyImage) mDestroyImage(mDevice, target.image, nullptr);
        if (target.memory && mFreeMemory) mFreeMemory(mDevice, target.memory, nullptr);
        if (target.ahb) AHardwareBuffer_release(target.ahb);
        target = {};
    }

    void clearTargetCache() {
        for (auto& [_, target] : mTargetCache) destroyTarget(target);
        mTargetCache.clear();
    }

    // Destroy a cached target and remove it from the cache. Used on blit error paths so a target
    // that failed mid-record/submit is not left dangling in the cache. Safe to call with a target
    // that is not (or no longer) cached.
    void evictTarget(TargetImage& target) {
        AHardwareBuffer* ahb = target.ahb;
        destroyTarget(target);
        if (ahb) mTargetCache.erase(ahb);
    }

    // Returns a cached Vulkan import for the dequeued AHardwareBuffer, creating it on first use.
    // BufferQueue recycles a small fixed set of AHBs, so after warm-up this avoids the per-frame
    // vkCreateImage/vkAllocateMemory/vkImportAndroidHardwareBuffer/vkDestroy churn. The returned
    // pointer is stable across map growth (unordered_map guarantees reference stability). Cache
    // Entries are freed only after draining the device: on dimension/surface change, cache-cap
    // reset, or teardown. Therefore an in-flight blit can never reference a destroyed target.
    Result<TargetImage*> acquireTargetImage(AHardwareBuffer* ahb, uint32_t width, uint32_t height) {
        // A geometry change makes every cached AHB stale. Drain in-flight work, then rebuild.
        if (mTargetCacheWidth != width || mTargetCacheHeight != height) {
            if (mDeviceWaitIdle) mDeviceWaitIdle(mDevice);
            for (auto& slot : mInFlightSlots) reclaimSlotAfterDeviceIdle(slot);
            clearTargetCache();
            mTargetCacheWidth = width;
            mTargetCacheHeight = height;
        }

        auto it = mTargetCache.find(ahb);
        if (it != mTargetCache.end()) return &it->second;

        // Bound the cache: BufferQueue normally holds 3-4 buffers, but a surface churn could leak
        // stale AHB pointers. If we exceed the cap, drain and reset rather than grow unbounded.
        if (mTargetCache.size() >= kTargetCacheCap) {
            if (mDeviceWaitIdle) mDeviceWaitIdle(mDevice);
            for (auto& slot : mInFlightSlots) reclaimSlotAfterDeviceIdle(slot);
            clearTargetCache();
        }

        TargetImage target{};
        {
            ScopedTrace targetTrace("crosvm_display.target_import");
            if (!createTargetImage(ahb, width, height, target)) {
                destroyTarget(target);
                return Error() << "Failed to import display target AHardwareBuffer";
            }
        }
        AHardwareBuffer_acquire(ahb);
        target.ahb = ahb;
        target.width = width;
        target.height = height;
        auto [inserted, _] = mTargetCache.emplace(ahb, target);
        return &inserted->second;
    }

    void destroyImport(ImportedImage& imported) {
        if (imported.sourceImage && mDestroyImage) {
            mDestroyImage(mDevice, imported.sourceImage, nullptr);
        }
        if (imported.sourceMemory && mFreeMemory) {
            mFreeMemory(mDevice, imported.sourceMemory, nullptr);
        }
        imported = {};
    }

    // CPU-side wait on an acquire fence; consumes (closes) the fd. Used only as the fallback when
    // vkImportSemaphoreFdKHR fails after the caller already handed the fence to blit().
    static Result<void> pollFence(int fenceFd) {
        if (fenceFd < 0) return {};
        pollfd descriptor{.fd = fenceFd, .events = POLLIN, .revents = 0};
        int result;
        do {
            result = poll(&descriptor, 1, -1);
        } while (result < 0 && errno == EINTR);
        close(fenceFd);
        if (result <= 0) return Error() << "Failed to wait for display acquire fence";
        return {};
    }

    bool mReady = false;
    static constexpr uint32_t kAsyncInFlightSlotCount = 3;
    static constexpr size_t kTargetCacheCap = 8;
    bool mAsyncBlitEnabled = false;
    bool mAsyncAcquireImportSupported = false;
    bool mSyncFdImportable = false;
    std::unordered_map<AHardwareBuffer*, TargetImage> mTargetCache;
    uint32_t mTargetCacheWidth = 0;
    uint32_t mTargetCacheHeight = 0;
    void* mLibrary = nullptr;
    HwvulkanDevice* mHal = nullptr;
    VkInstance mInstance = VK_NULL_HANDLE;
    VkPhysicalDevice mPhysicalDevice = VK_NULL_HANDLE;
    VkDevice mDevice = VK_NULL_HANDLE;
    VkQueue mQueue = VK_NULL_HANDLE;
    uint32_t mQueueFamilyIndex = 0;
    VkCommandPool mCommandPool = VK_NULL_HANDLE;
    std::vector<InFlightSlot> mInFlightSlots;
    size_t mNextInFlightSlot = 0;
    VkQueryPool mTimestampQueryPool = VK_NULL_HANDLE;
    float mTimestampPeriodNs = 0.0f;
    uint32_t mTimestampValidBits = 0;
    uint64_t mFrameSequence = 0;
    bool mTimestampReadFailureLogged = false;
    int64_t mNextImportId = 1;
    std::unordered_map<int64_t, ImportedImage> mImports;

    PFN_vkGetDeviceProcAddr mGetDeviceProcAddr = nullptr;
    PFN_vkGetPhysicalDeviceProperties mGetPhysicalDeviceProperties = nullptr;
    PFN_vkGetPhysicalDeviceFormatProperties2 mGetPhysicalDeviceFormatProperties2 = nullptr;
    PFN_vkGetPhysicalDeviceImageFormatProperties2 mGetPhysicalDeviceImageFormatProperties2 =
            nullptr;
    PFN_vkGetPhysicalDeviceExternalSemaphoreProperties
            mGetPhysicalDeviceExternalSemaphoreProperties = nullptr;
    PFN_vkDestroyInstance mDestroyInstance = nullptr;
    PFN_vkDestroyDevice mDestroyDevice = nullptr;
    PFN_vkDeviceWaitIdle mDeviceWaitIdle = nullptr;
    PFN_vkGetDeviceQueue mGetDeviceQueue = nullptr;
    PFN_vkCreateCommandPool mCreateCommandPool = nullptr;
    PFN_vkDestroyCommandPool mDestroyCommandPool = nullptr;
    PFN_vkAllocateCommandBuffers mAllocateCommandBuffers = nullptr;
    PFN_vkResetCommandBuffer mResetCommandBuffer = nullptr;
    PFN_vkBeginCommandBuffer mBeginCommandBuffer = nullptr;
    PFN_vkEndCommandBuffer mEndCommandBuffer = nullptr;
    PFN_vkQueueSubmit mQueueSubmit = nullptr;
    PFN_vkQueueWaitIdle mQueueWaitIdle = nullptr;
    PFN_vkCreateFence mCreateFence = nullptr;
    PFN_vkDestroyFence mDestroyFence = nullptr;
    PFN_vkWaitForFences mWaitForFences = nullptr;
    PFN_vkResetFences mResetFences = nullptr;
    PFN_vkCreateSemaphore mCreateSemaphore = nullptr;
    PFN_vkDestroySemaphore mDestroySemaphore = nullptr;
    PFN_vkGetSemaphoreFdKHR mGetSemaphoreFd = nullptr;
    PFN_vkImportSemaphoreFdKHR mImportSemaphoreFd = nullptr;
    PFN_vkCmdPipelineBarrier mCmdPipelineBarrier = nullptr;
    PFN_vkCmdBlitImage mCmdBlitImage = nullptr;
    PFN_vkCreateQueryPool mCreateQueryPool = nullptr;
    PFN_vkDestroyQueryPool mDestroyQueryPool = nullptr;
    PFN_vkCmdResetQueryPool mCmdResetQueryPool = nullptr;
    PFN_vkCmdWriteTimestamp mCmdWriteTimestamp = nullptr;
    PFN_vkGetQueryPoolResults mGetQueryPoolResults = nullptr;
    PFN_vkCreateImage mCreateImage = nullptr;
    PFN_vkDestroyImage mDestroyImage = nullptr;
    PFN_vkGetImageMemoryRequirements2 mGetImageMemoryRequirements2 = nullptr;
    PFN_vkAllocateMemory mAllocateMemory = nullptr;
    PFN_vkFreeMemory mFreeMemory = nullptr;
    PFN_vkBindImageMemory mBindImageMemory = nullptr;
    PFN_vkGetMemoryFdPropertiesKHR mGetMemoryFdProperties = nullptr;
    PFN_vkGetAndroidHardwareBufferPropertiesANDROID mGetAhbProperties = nullptr;
};

// A Vulkan blit with nothing on the far side of it.
//
// VulkanDisplayBridge above never needed a screen. Its inputs are a dmabuf fd and an
// AHardwareBuffer*, and everything screen-shaped -- the binder service, the app's Surface, the
// BufferQueue those AHBs are dequeued from -- lives in AndroidDisplaySurface beside it, not in it.
// This class is what is left of the flip path when those are taken away: the same bridge, the same
// import, the same blit, into a buffer allocated here and readable by the CPU.
//
// That is the shape the VNC sink needs (plan §6 step 11). It cannot present an AHB -- it has to put
// pixels on a socket -- so its GPU half is "let the GPU do the copy and the channel-order
// conversion, then read the result back", and the readback is the point rather than a cost to be
// engineered away. Step 13 wants the same machinery pointed at a MediaCodec input buffer instead;
// `ensureTarget` is the only member that knows what the target is for.
class HeadlessBlitContext {
public:
    // The geometry is where the target starts, not a contract: `blit` re-allocates when the source
    // turns out to be a different size, which is how a guest resolution change is absorbed. Doing
    // it here as well means a screen that never resizes pays for its buffer at open rather than on
    // its first frame.
    HeadlessBlitContext(uint32_t width, uint32_t height) {
        if (mBridge.ready() && width && height) ensureTarget(width, height);
    }

    ~HeadlessBlitContext() {
        unmap();
        releaseTarget();
        // mBridge is destroyed after this body and drains the device first, so the copy of our AHB
        // reference it holds in its target cache outlives the one released just above -- which is
        // also what stops a freshly allocated target from being handed back the same pointer.
    }

    HeadlessBlitContext(const HeadlessBlitContext&) = delete;
    HeadlessBlitContext& operator=(const HeadlessBlitContext&) = delete;

    bool ready() const { return mBridge.ready(); }

    // `exchangeRedBlue` picks which byte order the blits from this import land in, and it is a
    // property of the import rather than of the blit because the only lever there is -- the source
    // image's declared format -- is fixed when the image is created. Two consumers of the same
    // guest scanout therefore need two imports of it: LibVNCServer wants B,G,R,X (exchange), the
    // H.264 encoder reads its input as the RGBA_8888 gralloc says it is (no exchange). See
    // blitSourceFourcc for the arithmetic.
    int64_t importDmabuf(int fd, uint32_t offset, uint32_t stride, uint64_t modifier,
                         bool linearLayoutVerified, uint32_t width, uint32_t height, uint32_t fourcc,
                         bool exchangeRedBlue) {
        return mBridge.importDmabuf(fd, offset, stride, modifier, linearLayoutVerified, width,
                                    height,
                                    exchangeRedBlue ? blitSourceFourcc(fourcc) : fourcc);
    }

    void release(int64_t importId) { mBridge.release(importId); }

    // Blit an import into a target somebody else owns, and hand back the completion fence.
    //
    // This is `blit` above with the two halves that are about *this* context's own buffer taken
    // out: no ensureTarget, no unmap, and no wait. The caller supplies the AHardwareBuffer and
    // decides what the fence is for -- the H.264 path passes it straight to queueBuffer, so the
    // codec waits on the GPU instead of this thread doing it. -1 means the bridge already drained
    // the queue and the target is ready now.
    //
    // The bridge's target cache is keyed by AHB pointer, so a foreign target costs one Vulkan
    // import on first sight and nothing afterwards, exactly like a dequeued display buffer.
    Result<int> blitInto(int64_t importId, AHardwareBuffer* ahb) {
        if (!ahb) return Error() << "no target AHardwareBuffer to blit into";
        return mBridge.blit(importId, ahb);
    }

    // Blit an import into the target and wait, with a bound, for the GPU to be done with it.
    //
    // Returning drops any CPU mapping first: AHardwareBuffer_lock is where the CPU's view of this
    // memory is invalidated, so a mapping taken before this blit would go on showing the frame
    // before it. The caller maps again afterwards.
    Result<void> blit(int64_t importId, uint32_t width, uint32_t height, int timeoutMs) {
        unmap();
        if (!ensureTarget(width, height)) {
            return Error() << "no CPU-readable blit target for " << width << "x" << height;
        }
        auto fence = mBridge.blit(importId, mAhb);
        if (!fence.ok()) return fence.error();
        int fenceFd = *fence;
        // -1 is the bridge saying it already drained the queue itself, on either of the two paths
        // that end there: async blit disabled, or the completion sync_fd could not be exported.
        if (fenceFd < 0) return {};

        pollfd descriptor{.fd = fenceFd, .events = POLLIN, .revents = 0};
        int ret;
        do {
            ret = poll(&descriptor, 1, timeoutMs);
        } while (ret < 0 && errno == EINTR);
        const int pollErrno = errno;
        close(fenceFd);
        // Bounded, and a timeout is a hard error rather than "read it anyway". This is a readback,
        // not a present: handing the CPU a target the GPU is still writing produces a torn frame
        // with nothing anywhere to say so, which is exactly the class of failure that looks like a
        // picture. The caller's answer to an error is the CPU copy, which is always available.
        if (ret == 0) {
            return Error() << "blit completion fence unsignalled after " << timeoutMs << "ms";
        }
        if (ret < 0) return Error() << "poll on blit completion fence failed: " << pollErrno;
        return {};
    }

    // Map the target for CPU reading. The mapping stays valid until the next `blit` or until this
    // context is destroyed, which is what lets a consumer keep referring to the last frame -- the
    // VNC sink's cursor-only updates re-read the pixels the pointer used to cover.
    Result<void> map(const uint8_t** outPixels, uint32_t* outStrideBytes, uint32_t* outWidth,
                     uint32_t* outHeight, uint32_t* outSize) {
        if (!mAhb) return Error() << "no blit target to map";
        if (!mMapped) {
            void* address = nullptr;
            const int status = AHardwareBuffer_lock(mAhb, AHARDWAREBUFFER_USAGE_CPU_READ_OFTEN,
                                                    /* fence= */ -1, /* rect= */ nullptr, &address);
            if (status != 0 || address == nullptr) {
                return Error() << "AHardwareBuffer_lock for CPU read failed: " << status;
            }
            mAddress = address;
            mMapped = true;
        }
        *outPixels = static_cast<const uint8_t*>(mAddress);
        *outStrideBytes = mStrideBytes;
        *outWidth = mWidth;
        *outHeight = mHeight;
        *outSize = mStrideBytes * mHeight;
        return {};
    }

    void unmap() {
        if (!mMapped) return;
        AHardwareBuffer_unlock(mAhb, /* fence= */ nullptr);
        mMapped = false;
        mAddress = nullptr;
    }

private:
    static constexpr uint32_t fourccOf(char a, char b, char c, char d) {
        return static_cast<uint32_t>(a) | (static_cast<uint32_t>(b) << 8) |
                (static_cast<uint32_t>(c) << 16) | (static_cast<uint32_t>(d) << 24);
    }

    // The fourcc the SOURCE image is declared with, which is not the fourcc the guest declared.
    //
    // The only lever vkCmdBlitImage offers over channel order is the pair of formats it is given:
    // it reads texels through the source format and writes them through the destination format, so
    // naming a channel differently on one side moves it on the other. The destination here is
    // AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM -- R,G,B,A in memory, and the only 32-bit colour format
    // in the NDK's AHardwareBuffer enum at all, so it is not a choice. The consumer at the far end
    // is LibVNCServer, whose serverFormat is red<<16 green<<8 blue<<0 little-endian: B,G,R,X in
    // memory, the VNC sink's declared target order. Those two disagree by exactly a red/blue
    // exchange.
    //
    // The CPU path pays for that exchange at its producer-to-sink copy boundary. Here it costs
    // nothing at all: declare the source with its red and blue names exchanged and the GPU performs
    // the swap as part of the copy it was already doing. A guest AR24 scanout is B,G,R,A in memory;
    // declared as AB24 the blit samples R := B_true and
    // B := R_true, and writing that through an R8G8B8A8 destination puts B,G,R,A back in memory --
    // which is what VNC reads. Exactly, not approximately: identical extents with VK_FILTER_NEAREST
    // between two 8-bit UNORM formats is a texel copy.
    //
    // This is a deliberate misdeclaration in the one place §4.4 warns is silently fatal, so it is a
    // named function with the arithmetic written out rather than a swapped constant at a call site.
    // Backwards, it does not fail: it returns the whole picture with red and blue exchanged, with
    // nothing in any log.
    static uint32_t blitSourceFourcc(uint32_t guestFourcc) {
        switch (guestFourcc) {
            case fourccOf('A', 'R', '2', '4'):
                return fourccOf('A', 'B', '2', '4');
            case fourccOf('A', 'B', '2', '4'):
                return fourccOf('A', 'R', '2', '4');
            case fourccOf('X', 'R', '2', '4'):
                return fourccOf('X', 'B', '2', '4');
            case fourccOf('X', 'B', '2', '4'):
                return fourccOf('X', 'R', '2', '4');
            default:
                // Not a format the bridge accepts either way; hand it through so the refusal names
                // the fourcc the guest actually declared.
                return guestFourcc;
        }
    }

    bool ensureTarget(uint32_t width, uint32_t height) {
        if (mAhb && mWidth == width && mHeight == height) return true;
        unmap();
        releaseTarget();
        if (!width || !height) return false;

        AHardwareBuffer_Desc request = {
                .width = width,
                .height = height,
                .layers = 1,
                .format = AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM,
                // GPU_COLOR_OUTPUT is what makes gralloc give back something Vulkan will import as
                // a blit destination; CPU_READ_OFTEN is what this buffer exists for.
                .usage = AHARDWAREBUFFER_USAGE_CPU_READ_OFTEN |
                        AHARDWAREBUFFER_USAGE_GPU_COLOR_OUTPUT,
        };
        AHardwareBuffer* ahb = nullptr;
        const int status = AHardwareBuffer_allocate(&request, &ahb);
        if (status != 0 || ahb == nullptr) {
            LOG(ERROR) << "failed to allocate " << width << "x" << height
                       << " CPU-readable blit target: " << status;
            return false;
        }
        AHardwareBuffer_Desc actual{};
        AHardwareBuffer_describe(ahb, &actual);
        mAhb = ahb;
        mWidth = width;
        mHeight = height;
        // AHardwareBuffer_describe reports the stride in PIXELS. gralloc is free to pad it past the
        // width, and does; the consumer is told the real number rather than being assumed packed.
        mStrideBytes = actual.stride * 4;
        LOG(INFO) << "headless blit target " << width << "x" << height
                  << " stride=" << actual.stride << "px";
        return true;
    }

    void releaseTarget() {
        if (!mAhb) return;
        AHardwareBuffer_release(mAhb);
        mAhb = nullptr;
        mWidth = 0;
        mHeight = 0;
        mStrideBytes = 0;
    }

    VulkanDisplayBridge mBridge;
    AHardwareBuffer* mAhb = nullptr;
    uint32_t mWidth = 0;
    uint32_t mHeight = 0;
    uint32_t mStrideBytes = 0;
    bool mMapped = false;
    void* mAddress = nullptr;
};

// The guest's hardware cursor, as one frame's worth of overlay.
//
// The classic VNC consumer blends this into its own outgoing framebuffer (blend_cursor, in
// crosvm's vnc_server_bridge.c) and hands a copy to LibVNCServer as an RFB cursor so a client can
// draw it itself. Neither of those reaches an H.264 decoder: a video stream is pixels and nothing
// else, so for this consumer the pointer has to be IN the encoded picture or it is not there at
// all. Same bytes, same straight-alpha convention, blended a second time into a different canvas.
struct H264CursorOverlay {
    const uint8_t* pixels = nullptr; // B,G,R,A with a meaningful alpha byte
    int width = 0;
    int height = 0;
    int x = 0; // top-left of the image, hotspot already applied by the guest; may be negative
    int y = 0;
    bool visible = false;
};

// One H.264 encoder, fed through its own input Surface.
//
// Plan §6 step 13. The picture the VNC sink already has -- the same frame, at the same instant,
// off the same bus offer -- goes into a MediaCodec input buffer instead of onto an RFB socket, and
// the compressed result leaves by a side channel. RFB is untouched; a legacy client keeps being
// served by the LibVNCServer consumer from the same offer, which is what the bus was split for.
//
// The input Surface is the whole reason this is worth doing. AMediaCodec_createInputSurface hands
// back an ANativeWindow whose buffers are gralloc buffers, so the frame can be delivered to the
// encoder the same way it is delivered to the app's display: dequeue a buffer, put the picture in
// it, queue it. When the producer is on the GPU transport that "put the picture in it" is the
// step-11 Vulkan blit with a different target, and the guest's pixels never touch the CPU on their
// way into the codec. Plan §7 listed that as an unverified premise -- "createTargetImage takes any
// AHardwareBuffer, mechanically it should work" -- and this class is where it is either true or
// reported false.
//
// The CPU route beside it is not a second design. It exists because the offer does not always come
// with a GPU source (a simplefb producer that fell back, a resource that failed to import), and
// because a dequeued codec buffer has to be CPU-writable anyway for the cursor to be blended into
// it. One dequeue/queue path, two ways to fill the buffer.
class H264EncoderSession {
public:
    static std::unique_ptr<H264EncoderSession> Open(uint32_t width, uint32_t height,
                                                    int32_t bitrateBps, int32_t frameRate,
                                                    int32_t iFrameIntervalSecs) {
        auto& media = MediaCodecLib::GetInstance();
        if (!media.IsSupported()) {
            LOG(ERROR) << "H.264 side channel: no media NDK on this device";
            return nullptr;
        }
        if (!width || !height) {
            LOG(ERROR) << "H.264 side channel: refusing a " << width << "x" << height << " screen";
            return nullptr;
        }

        // MediaCodec is a binder client, and the buffers it hands back arrive on incoming binder
        // transactions -- so this process needs threads to receive them on. The display path
        // starts the pool before registering its service; a VNC-only crosvm never takes that path,
        // and without this the codec comes up and then never returns a buffer. Idempotent, so the
        // two callers do not have to know about each other.
        ABinderProcess_setThreadPoolMaxThreadCount(4);
        ABinderProcess_startThreadPool();

        auto session = std::unique_ptr<H264EncoderSession>(new H264EncoderSession(width, height));

        AMediaFormat* format = media.AMediaFormat_new();
        if (!format) {
            LOG(ERROR) << "H.264 side channel: AMediaFormat_new failed";
            return nullptr;
        }
        auto formatGuard = android::base::make_scope_guard(
                [&] { media.AMediaFormat_delete(format); });
        media.AMediaFormat_setString(format, kFormatKeyMime, kMimeTypeAvc);
        media.AMediaFormat_setInt32(format, kFormatKeyWidth, static_cast<int32_t>(width));
        media.AMediaFormat_setInt32(format, kFormatKeyHeight, static_cast<int32_t>(height));
        media.AMediaFormat_setInt32(format, kFormatKeyColorFormat, kColorFormatSurface);
        media.AMediaFormat_setInt32(format, kFormatKeyBitRate, bitrateBps);
        media.AMediaFormat_setInt32(format, kFormatKeyFrameRate, frameRate);
        media.AMediaFormat_setInt32(format, kFormatKeyIFrameInterval, iFrameIntervalSecs);

        session->mCodec = media.AMediaCodec_createEncoderByType(kMimeTypeAvc);
        if (!session->mCodec) {
            LOG(ERROR) << "H.264 side channel: no encoder for " << kMimeTypeAvc;
            return nullptr;
        }
        media_status_t status = media.AMediaCodec_configure(session->mCodec, format,
                                                            /* surface= */ nullptr,
                                                            /* crypto= */ nullptr,
                                                            AMEDIACODEC_CONFIGURE_FLAG_ENCODE);
        if (status != AMEDIA_OK) {
            LOG(ERROR) << "H.264 side channel: configure(" << width << "x" << height << " @"
                       << bitrateBps << "bps) failed: " << status;
            return nullptr;
        }
        status = media.AMediaCodec_createInputSurface(session->mCodec, &session->mWindow);
        if (status != AMEDIA_OK || session->mWindow == nullptr) {
            LOG(ERROR) << "H.264 side channel: createInputSurface failed: " << status;
            return nullptr;
        }
        status = media.AMediaCodec_start(session->mCodec);
        if (status != AMEDIA_OK) {
            LOG(ERROR) << "H.264 side channel: start failed: " << status;
            return nullptr;
        }
        if (auto ready = session->prepareWindow(); !ready.ok()) {
            LOG(ERROR) << "H.264 side channel: " << ready.error().message();
            return nullptr;
        }

        std::string codecName = "?";
        if (media.AMediaCodec_getName && media.AMediaCodec_releaseName) {
            char* name = nullptr;
            if (media.AMediaCodec_getName(session->mCodec, &name) == AMEDIA_OK && name) {
                codecName = name;
                media.AMediaCodec_releaseName(session->mCodec, name);
            }
        }
        LOG(INFO) << "H.264 side channel: encoder \"" << codecName << "\" up for " << width << "x"
                  << height << " @ " << bitrateBps << " bps, " << frameRate << " fps, IDR every "
                  << iFrameIntervalSecs << "s";
        return session;
    }

    ~H264EncoderSession() {
        auto& media = MediaCodecLib::GetInstance();
        if (mCodec) {
            if (mWindow) media.AMediaCodec_signalEndOfInputStream(mCodec);
            media.AMediaCodec_stop(mCodec);
            media.AMediaCodec_delete(mCodec);
        }
        if (mWindow) {
            if (mConnectedApi) native_window_api_disconnect(mWindow, mConnectedApi);
            ANativeWindow_release(mWindow);
        }
    }

    H264EncoderSession(const H264EncoderSession&) = delete;
    H264EncoderSession& operator=(const H264EncoderSession&) = delete;

    uint32_t width() const { return mWidth; }
    uint32_t height() const { return mHeight; }

    // Ask the encoder for an IDR at the next opportunity.
    //
    // Called when a side-channel client connects, which is the case the i-frame interval cannot
    // cover: a decoder that joins between two IDRs has no reference frame and shows nothing until
    // the next one, up to a whole interval later. Cheap enough that there is no reason to be
    // clever about it -- the encoder coalesces repeats itself.
    void requestSyncFrame() {
        auto& media = MediaCodecLib::GetInstance();
        AMediaFormat* params = media.AMediaFormat_new();
        if (!params) return;
        media.AMediaFormat_setInt32(params, kFormatKeyRequestSync, 0);
        media.AMediaCodec_setParameters(mCodec, params);
        media.AMediaFormat_delete(params);
    }

    // Feed one picture into the codec's input surface.
    //
    // `importId` non-zero selects the GPU route: `blitCtx` blits that import straight into the
    // dequeued gralloc buffer. Zero selects the CPU route, which uploads `pixels` (B,G,R,X, packed
    // to `width`) into it. Either way the cursor is blended afterwards, in place, on the CPU.
    Result<void> encodeFrame(HeadlessBlitContext* blitCtx, int64_t importId, const uint8_t* pixels,
                             uint32_t pixelsSize, uint32_t width, uint32_t height,
                             const H264CursorOverlay& cursor, int64_t ptsUs) {
        if (width != mWidth || height != mHeight) {
            return Error() << "encoder is " << mWidth << "x" << mHeight << " but the frame is "
                           << width << "x" << height;
        }
        if (auto ready = prepareWindow(); !ready.ok()) return ready;

        // The timestamp has to be set before the buffer is queued: it travels with the queue, not
        // with the dequeue. A codec input surface reads it as the frame's presentation time.
        ANativeWindow_setBuffersTimestamp(mWindow, ptsUs * 1000);

        ANativeWindowBuffer* buffer = nullptr;
        int fenceFd = -1;
        int result = ANativeWindow_dequeueBuffer(mWindow, &buffer, &fenceFd);
        if (result == -ETIMEDOUT || result == -EWOULDBLOCK) {
            // Every input buffer is still with the encoder. Drop the frame rather than stall: this
            // runs on the guest's own flush path (VncSurface::flip_to), so waiting here would put
            // a video encoder into the guest's vblank loop -- the exact coupling step 11 measured
            // its way out of. The next flush carries a newer picture anyway.
            if (fenceFd >= 0) close(fenceFd);
            ++mDroppedFrames;
            return {};
        }
        if (result != 0 || buffer == nullptr) {
            if (fenceFd >= 0) close(fenceFd);
            return Error() << "dequeue from the codec input surface failed: " << result;
        }

        AHardwareBuffer* ahb = ANativeWindowBuffer_getHardwareBuffer(buffer);
        if (ahb == nullptr) {
            if (fenceFd >= 0) close(fenceFd);
            ANativeWindow_cancelBuffer(mWindow, buffer, -1);
            return Error() << "the codec input buffer has no AHardwareBuffer";
        }

        // The GPU route hands the acquire fence to the blit and gets a completion fence back; the
        // CPU route has to wait for the acquire itself before it may touch the memory.
        int queueFenceFd = -1;
        Result<void> filled;
        if (importId != 0 && blitCtx != nullptr) {
            filled = fillByBlit(blitCtx, importId, ahb, fenceFd, &queueFenceFd);
            fenceFd = -1; // fillByBlit owns it on every path
        } else {
            filled = waitFence(fenceFd);
            fenceFd = -1; // waitFence closes it
            if (filled.ok()) filled = fillByUpload(ahb, pixels, pixelsSize);
        }
        if (!filled.ok()) {
            if (queueFenceFd >= 0) close(queueFenceFd);
            ANativeWindow_cancelBuffer(mWindow, buffer, -1);
            return filled;
        }

        if (cursor.visible && cursor.pixels && cursor.width > 0 && cursor.height > 0) {
            // The blend reads and writes the target, so whatever is still writing it has to be
            // finished. On the CPU route that fence was already waited on and this is a no-op.
            if (auto ret = waitFence(queueFenceFd); !ret.ok()) {
                queueFenceFd = -1;
                ANativeWindow_cancelBuffer(mWindow, buffer, -1);
                return ret;
            }
            queueFenceFd = -1;
            if (auto ret = blendCursor(ahb, cursor); !ret.ok()) {
                ANativeWindow_cancelBuffer(mWindow, buffer, -1);
                return ret;
            }
        }

        result = ANativeWindow_queueBuffer(mWindow, buffer, queueFenceFd);
        if (result != 0) {
            // queueBuffer consumes the fence even when it fails, so it is not ours to close.
            ANativeWindow_cancelBuffer(mWindow, buffer, -1);
            return Error() << "queue to the codec input surface failed: " << result;
        }
        ++mQueuedFrames;
        return {};
    }

    // Drain one compressed buffer.
    //
    // Returns the number of bytes written to `out`, 0 when nothing was ready before the timeout,
    // and a negative number on error. `kOutputTooSmall` means the buffer exists but did not fit;
    // *outSize then holds what it needs and the encoded frame has NOT been dropped -- it is still
    // owned by the codec, so a caller that grows and asks again gets the same one.
    static constexpr int32_t kOutputTooSmall = -1;
    static constexpr int32_t kOutputFailed = -2;
    int32_t pollOutput(uint8_t* out, uint32_t cap, uint32_t* outSize, uint32_t* outFlags,
                       int64_t* outPtsUs, int64_t timeoutUs) {
        auto& media = MediaCodecLib::GetInstance();
        AMediaCodecBufferInfo info{};
        ssize_t index = media.AMediaCodec_dequeueOutputBuffer(mCodec, &info, timeoutUs);
        if (index == AMEDIACODEC_INFO_TRY_AGAIN_LATER ||
            index == AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED) {
            return 0;
        }
        if (index == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED) {
            captureCodecConfig();
            return 0;
        }
        if (index < 0) return kOutputFailed;

        size_t bufferSize = 0;
        uint8_t* data = media.AMediaCodec_getOutputBuffer(mCodec, static_cast<size_t>(index),
                                                          &bufferSize);
        if (data == nullptr) {
            media.AMediaCodec_releaseOutputBuffer(mCodec, static_cast<size_t>(index), false);
            return kOutputFailed;
        }
        const uint32_t size = static_cast<uint32_t>(info.size < 0 ? 0 : info.size);
        if (outSize) *outSize = size;
        if (outFlags) *outFlags = info.flags;
        if (outPtsUs) *outPtsUs = info.presentationTimeUs;
        if (size > cap) {
            // Left undrained on purpose: releasing it here would lose the frame, and the caller's
            // answer to "it did not fit" is to come back with a bigger buffer.
            return kOutputTooSmall;
        }
        memcpy(out, data + info.offset, size);
        if ((info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG) != 0) {
            std::lock_guard lk(mCodecConfigMutex);
            mCodecConfig.assign(out, out + size);
        }
        media.AMediaCodec_releaseOutputBuffer(mCodec, static_cast<size_t>(index), false);
        return static_cast<int32_t>(size);
    }

    // The stream's SPS and PPS, as Annex-B, or nothing if the encoder has not produced them yet.
    //
    // Kept so a client that connects mid-stream can be handed them before its first IDR. The
    // encoder emits them exactly once, in the first output buffer, which is fine for the client
    // that was already connected and useless for every one after it.
    size_t codecConfig(uint8_t* out, uint32_t cap) {
        std::lock_guard lk(mCodecConfigMutex);
        if (mCodecConfig.empty() || mCodecConfig.size() > cap) return mCodecConfig.size();
        memcpy(out, mCodecConfig.data(), mCodecConfig.size());
        return mCodecConfig.size();
    }

    uint64_t droppedFrames() const { return mDroppedFrames; }
    uint64_t queuedFrames() const { return mQueuedFrames; }

private:
    H264EncoderSession(uint32_t width, uint32_t height) : mWidth(width), mHeight(height) {}

    // What the input surface's buffers have to be for both fill routes to work.
    //
    // RGBA_8888 because it is what an encoder input surface is fed everywhere else in Android
    // (SurfaceFlinger's own screen recording path) and the only 32-bit colour format Vulkan will
    // import an AHardwareBuffer as. GPU_COLOR_OUTPUT is what makes gralloc give back something the
    // blit can be a destination of; the CPU bits are for the cursor blend and the upload route.
    // Asking for CPU access also tends to force a linear layout, which is what the Vulkan import
    // wants anyway -- a compressed one would be refused.
    Result<void> prepareWindow() {
        if (mWindowReady) return {};
        if (mWindow == nullptr) return Error() << "no codec input surface";
        // CPU is the honest declaration -- this producer writes with the CPU on one route and with
        // a Vulkan blit on the other, and neither is EGL. EGL is tried second only because it is
        // what every other producer of a codec input surface in Android connects as, so a
        // BufferQueue that turns CPU down is not out of the question and the failure would
        // otherwise be a whole rung reported as unavailable.
        mConnectedApi = NATIVE_WINDOW_API_CPU;
        int connected = native_window_api_connect(mWindow, mConnectedApi);
        if (connected != 0) {
            LOG(WARNING) << "codec input surface refused a CPU producer (" << connected
                         << "); trying EGL";
            mConnectedApi = NATIVE_WINDOW_API_EGL;
            connected = native_window_api_connect(mWindow, mConnectedApi);
        }
        if (connected != 0) {
            mConnectedApi = 0;
            return Error() << "failed to connect to the codec input surface: " << connected;
        }
        if (int result = ANativeWindow_setBuffersGeometry(mWindow, static_cast<int32_t>(mWidth),
                                                          static_cast<int32_t>(mHeight),
                                                          HAL_PIXEL_FORMAT_RGBA_8888);
            result != 0) {
            native_window_api_disconnect(mWindow, mConnectedApi);
            mConnectedApi = 0;
            return Error() << "failed to set codec input geometry: " << result;
        }
        constexpr uint64_t kUsage = AHARDWAREBUFFER_USAGE_GPU_COLOR_OUTPUT |
                AHARDWAREBUFFER_USAGE_CPU_WRITE_OFTEN | AHARDWAREBUFFER_USAGE_CPU_READ_OFTEN;
        if (int result = ANativeWindow_setUsage(mWindow, kUsage); result != 0) {
            native_window_api_disconnect(mWindow, mConnectedApi);
            mConnectedApi = 0;
            return Error() << "failed to set codec input usage: " << result;
        }
        // Never block the producer. Same reason as the display sink's GPU path: the caller is on
        // the guest's flush path and a full codec queue must cost a dropped frame, not a stalled
        // guest.
        if (int result = mWindow->perform(mWindow, NATIVE_WINDOW_SET_DEQUEUE_TIMEOUT, (int64_t)0);
            result != 0) {
            LOG(WARNING) << "H.264 side channel: zero dequeue timeout refused: " << result;
        }
        mWindowReady = true;
        return {};
    }

    // Waits for a fence and closes it. -1 is "nothing to wait for" and succeeds.
    static Result<void> waitFence(int fenceFd) {
        if (fenceFd < 0) return {};
        pollfd descriptor{.fd = fenceFd, .events = POLLIN, .revents = 0};
        int ret;
        do {
            ret = poll(&descriptor, 1, kFenceTimeoutMs);
        } while (ret < 0 && errno == EINTR);
        const int pollErrno = errno;
        close(fenceFd);
        if (ret == 0) return Error() << "codec buffer fence unsignalled after " << kFenceTimeoutMs
                                     << "ms";
        if (ret < 0) return Error() << "poll on a codec buffer fence failed: " << pollErrno;
        return {};
    }

    // The GPU route: the guest's own dmabuf, blitted into the encoder's buffer by the same Vulkan
    // bridge that blits into the app's display buffer. This is the §7 premise; if it is going to
    // fail it fails here, and the message says so.
    Result<void> fillByBlit(HeadlessBlitContext* blitCtx, int64_t importId, AHardwareBuffer* ahb,
                            int acquireFenceFd, int* outCompletionFd) {
        // The bridge's blit does not take an acquire fence on this entry point, so the wait is
        // ours. It is a display buffer coming back from the encoder rather than from a compositor,
        // so in practice it is already signalled.
        if (auto ret = waitFence(acquireFenceFd); !ret.ok()) return ret;
        auto fence = blitCtx->blitInto(importId, ahb);
        if (!fence.ok()) {
            return Error() << "Vulkan blit into the MediaCodec input buffer failed: "
                           << fence.error().message();
        }
        *outCompletionFd = *fence;
        return {};
    }

    // The CPU route: the picture the sink already has, copied in with red and blue exchanged.
    //
    // `pixels` is the VNC sink's B,G,R,X framebuffer and the MediaCodec input buffer is declared
    // RGBA_8888. Convert while uploading, matching the producer-to-sink edge rule used by the
    // crosvm CPU display path.
    Result<void> fillByUpload(AHardwareBuffer* ahb, const uint8_t* pixels, uint32_t pixelsSize) {
        if (pixels == nullptr) return Error() << "no pixels to upload and no GPU source either";
        AHardwareBuffer_Desc desc{};
        AHardwareBuffer_describe(ahb, &desc);
        void* address = nullptr;
        const int status = AHardwareBuffer_lock(ahb, AHARDWAREBUFFER_USAGE_CPU_WRITE_OFTEN,
                                                /* fence= */ -1, /* rect= */ nullptr, &address);
        if (status != 0 || address == nullptr) {
            return Error() << "AHardwareBuffer_lock on a MediaCodec input buffer failed: "
                           << status;
        }
        const size_t dstStride = static_cast<size_t>(desc.stride) * 4;
        const size_t srcStride = static_cast<size_t>(mWidth) * 4;
        uint8_t* dstBase = static_cast<uint8_t*>(address);
        for (uint32_t y = 0; y < mHeight; y++) {
            const size_t srcOff = static_cast<size_t>(y) * srcStride;
            if (srcOff + srcStride > pixelsSize) break;
            copyBgrxRowToRgba(dstBase + static_cast<size_t>(y) * dstStride, pixels + srcOff,
                              mWidth);
        }
        AHardwareBuffer_unlock(ahb, /* fence= */ nullptr);
        return {};
    }

    static void copyBgrxRowToRgba(uint8_t* dst, const uint8_t* src, uint32_t pixelCount) {
        uint32_t x = 0;
#if defined(__aarch64__)
        for (; x + 16 <= pixelCount; x += 16, dst += 64, src += 64) {
            uint8x16x4_t channels = vld4q_u8(src);
            uint8x16_t tmp = channels.val[0];
            channels.val[0] = channels.val[2];
            channels.val[2] = tmp;
            vst4q_u8(dst, channels);
        }
#endif
        for (; x < pixelCount; x++, dst += 4, src += 4) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
    }

    // Alpha-blend the guest's pointer into the encoded picture, clipped to the screen.
    //
    // Straight (non-premultiplied) alpha, matching blend_cursor on the classic path exactly, so
    // the two renderings of the same pointer agree. The source is B,G,R,A and the target is
    // R,G,B,A, so the channel exchange happens here too.
    Result<void> blendCursor(AHardwareBuffer* ahb, const H264CursorOverlay& cursor) {
        const int screenW = static_cast<int>(mWidth);
        const int screenH = static_cast<int>(mHeight);
        const int x0 = cursor.x < 0 ? 0 : cursor.x;
        const int y0 = cursor.y < 0 ? 0 : cursor.y;
        const int x1 = std::min(cursor.x + cursor.width, screenW);
        const int y1 = std::min(cursor.y + cursor.height, screenH);
        if (x1 <= x0 || y1 <= y0) return {};

        AHardwareBuffer_Desc desc{};
        AHardwareBuffer_describe(ahb, &desc);
        ARect rect{.left = x0, .top = y0, .right = x1, .bottom = y1};
        void* address = nullptr;
        const int status = AHardwareBuffer_lock(
                ahb, AHARDWAREBUFFER_USAGE_CPU_WRITE_OFTEN | AHARDWAREBUFFER_USAGE_CPU_READ_OFTEN,
                /* fence= */ -1, &rect, &address);
        if (status != 0 || address == nullptr) {
            return Error() << "AHardwareBuffer_lock for the cursor blend failed: " << status;
        }
        const size_t dstStride = static_cast<size_t>(desc.stride) * 4;
        uint8_t* dstBase = static_cast<uint8_t*>(address);
        for (int y = y0; y < y1; y++) {
            const uint8_t* src =
                    cursor.pixels + (((y - cursor.y) * cursor.width) + (x0 - cursor.x)) * 4;
            uint8_t* dst = dstBase + static_cast<size_t>(y) * dstStride +
                    static_cast<size_t>(x0) * 4;
            for (int x = x0; x < x1; x++, src += 4, dst += 4) {
                const uint32_t a = src[3];
                if (a == 0) continue;
                if (a == 255) {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    continue;
                }
                dst[0] = static_cast<uint8_t>((src[2] * a + dst[0] * (255 - a)) / 255);
                dst[1] = static_cast<uint8_t>((src[1] * a + dst[1] * (255 - a)) / 255);
                dst[2] = static_cast<uint8_t>((src[0] * a + dst[2] * (255 - a)) / 255);
            }
        }
        AHardwareBuffer_unlock(ahb, /* fence= */ nullptr);
        return {};
    }

    // SPS and PPS off the settled output format, concatenated in that order.
    //
    // Belt and braces with the CODEC_CONFIG output buffer pollOutput also caches: which of the two
    // an encoder produces is not something every component agrees on, and both arrive before the
    // first coded picture, so whichever lands first is the one a late client is handed.
    void captureCodecConfig() {
        auto& media = MediaCodecLib::GetInstance();
        AMediaFormat* format = media.AMediaCodec_getOutputFormat(mCodec);
        if (!format) return;
        auto formatGuard =
                android::base::make_scope_guard([&] { media.AMediaFormat_delete(format); });
        std::vector<uint8_t> config;
        for (const char* key : {kFormatKeyCsd0, kFormatKeyCsd1}) {
            void* data = nullptr;
            size_t size = 0;
            if (media.AMediaFormat_getBuffer(format, key, &data, &size) && data && size) {
                const uint8_t* bytes = static_cast<const uint8_t*>(data);
                config.insert(config.end(), bytes, bytes + size);
            }
        }
        if (config.empty()) return;
        std::lock_guard lk(mCodecConfigMutex);
        if (mCodecConfig.empty()) {
            LOG(INFO) << "H.264 side channel: codec config is " << config.size()
                      << " bytes of Annex-B";
            mCodecConfig = std::move(config);
        }
    }

    // Three vsyncs of a 120 Hz panel, the figure every other bounded fence wait in this file uses.
    static constexpr int kFenceTimeoutMs = 25;

    const uint32_t mWidth;
    const uint32_t mHeight;
    AMediaCodec* mCodec = nullptr;
    ANativeWindow* mWindow = nullptr;
    bool mWindowReady = false;
    /// Which producer API the input surface is connected as, or 0 for not connected.
    int mConnectedApi = 0;
    uint64_t mDroppedFrames = 0;
    uint64_t mQueuedFrames = 0;
    std::mutex mCodecConfigMutex;
    std::vector<uint8_t> mCodecConfig;
};

class SinkANativeWindow_Buffer {
public:
    Result<void> configure(uint32_t width, uint32_t height, int format) {
        // kFormat is RGBA_8888 (see AndroidDisplaySurface::kFormat for why we no longer use BGRA).
        if (format != HAL_PIXEL_FORMAT_RGBA_8888) {
            return Error() << "Pixel format " << format << " is not RGBA_8888.";
        }

        mBufferBits.resize(width * height * 4);
        mBuffer = ANativeWindow_Buffer{
                .width = static_cast<int32_t>(width),
                .height = static_cast<int32_t>(height),
                .stride = static_cast<int32_t>(width),
                .format = format,
                .bits = mBufferBits.data(),
        };
        return {};
    }

    operator ANativeWindow_Buffer&() { return mBuffer; }

private:
    ANativeWindow_Buffer mBuffer;
    std::vector<uint8_t> mBufferBits;
};

// The acceptance instrument for a byte-identical refactor of the display pipeline (see the plan's
// §6 step 4 and §9): the three CPU copy sites that feed this sink are being consolidated behind one
// function, and "the sink receives the same bytes" is not an acceptance condition until something
// measures it. This sits deliberately below everything that refactor touches, so the same number is
// taken in both binaries and the two frame sequences can simply be compared.
//
// The hash covers exactly the visible pixels, row by row: the padding a gralloc stride leaves at
// the end of each row is displayed by nobody and initialised by nothing, so hashing it would let
// two identical frames disagree. Off unless CROSVM_DISPLAY_HASH_FRAMES=1, read once.
static bool frameHashEnabled() {
    static const bool enabled = envFlagEnabled("CROSVM_DISPLAY_HASH_FRAMES", false);
    return enabled;
}

static uint64_t fnv1a64VisiblePixels(const ANativeWindow_Buffer& buf) {
    uint64_t hash = 0xcbf29ce484222325ULL;
    const uint8_t* base = static_cast<const uint8_t*>(buf.bits);
    if (base == nullptr || buf.width <= 0 || buf.height <= 0) return hash;
    const size_t rowBytes = static_cast<size_t>(buf.width) * 4;
    for (int32_t y = 0; y < buf.height; y++) {
        const uint8_t* px = base + static_cast<size_t>(y) * static_cast<size_t>(buf.stride) * 4;
        for (size_t i = 0; i < rowBytes; i++) {
            hash = (hash ^ px[i]) * 0x100000001b3ULL;
        }
    }
    return hash;
}

static void logFrameHash(const std::string& surfaceName, const ANativeWindow_Buffer& buf) {
    LOG(INFO) << "FRAMEHASH surface=" << surfaceName << " " << buf.width << "x" << buf.height
              << " fnv1a64=0x" << std::hex << std::setw(16) << std::setfill('0')
              << fnv1a64VisiblePixels(buf) << std::dec;
}

static Result<void> copyBuffer(ANativeWindow_Buffer& from, ANativeWindow_Buffer& to) {
    if (from.width != to.width || from.height != to.height) {
        return Error() << "dimension mismatch. from=(" << from.width << ", " << from.height << ") "
                       << "to=(" << to.width << ", " << to.height << ")";
    }
    uint32_t* dst = reinterpret_cast<uint32_t*>(to.bits);
    uint32_t* src = reinterpret_cast<uint32_t*>(from.bits);
    size_t bytes_on_line = to.width * 4; // 4 bytes per pixel
    for (int32_t h = 0; h < to.height; h++) {
        memcpy(dst + (h * to.stride), src + (h * from.stride), bytes_on_line);
    }
    return {};
}

// Wrapper which contains the latest available Surface/ANativeWindow from the DisplayService, if
// available. A Surface/ANativeWindow may not always be available if, for example, the VmLauncherApp
// on the other end of the DisplayService is not in the foreground / is paused.
class AndroidDisplaySurface {
public:
    AndroidDisplaySurface(const std::string& name) : mName(name) {}

    Result<void> setNativeSurface(Surface* surface) {
        {
            std::lock_guard lk(mSurfaceMutex);
            LOG(INFO) << "display surface " << mName << " native window attached";
            clearNativeSurfaceLocked(true);
            mNativeSurface = std::make_unique<Surface>(surface->release());
            mNativeSurfaceNeedsConfiguring = true;
            ++mNativeSurfaceGeneration;
            Surface* surface = mNativeSurface.get();
            if (!surface) {
                return Error() << "Failed to get Surface";
            }

            ANativeWindow* anw = surface->get();
            auto& sc = SurfaceControl::GetInstance();
            if (sc.IsSupported()) {
                mSurfaceControl = sc.ASurfaceControl_createFromWindow(anw, mName.c_str());
            }
        }

        mNativeSurfaceReady.notify_one();
        return {};
    }

    void removeSurface() {
        {
            std::lock_guard lk(mSurfaceMutex);
            clearNativeSurfaceLocked(true);
        }
        mNativeSurfaceReady.notify_one();
    }

    Surface* getSurface() {
        std::unique_lock lk(mSurfaceMutex);
        return mNativeSurface.get();
    }

    // Whether a native window is attached right now. This is the non-blocking read of the same
    // field that waitForNativeSurface() parks on, and the distinction is the whole point: the
    // caller is crosvm's simplefb bridge asking once per frame from its 30 fps timer thread, and
    // waiting there would hold that thread -- input dispatch included -- until the user came back
    // to the display view.
    bool hasNativeSurface() {
        std::lock_guard lk(mSurfaceMutex);
        return mNativeSurface != nullptr;
    }

    Result<void> configure(uint32_t width, uint32_t height) {
        std::unique_lock lk(mSurfaceMutex);

        LOG(INFO) << "display surface " << mName << " configure " << width << "x" << height;
        mRequestedSurfaceDimensions = Rect{
                .width = width,
                .height = height,
        };
        // The buffer geometry must follow: if a native window is already attached, its buffers
        // were sized for the previous dimensions and the next lock() has to re-apply them, or
        // frames of the new size get scaled into stale-geometry buffers (squashed display).
        mNativeSurfaceNeedsConfiguring = true;

        if (auto ret = mSinkBuffer.configure(width, height, kFormat); !ret.ok()) {
            return Error() << "Failed to configure sink buffer: " << ret.error();
        }
        if (auto ret = mSavedFrameBuffer.configure(width, height, kFormat); !ret.ok()) {
            return Error() << "Failed to configure saved frame buffer: " << ret.error();
        }
        return {};
    }

    void waitForNativeSurface() {
        std::unique_lock lk(mSurfaceMutex);
        mNativeSurfaceReady.wait(lk, [this] { return mNativeSurface != nullptr; });
    }

    Result<void> lock(ANativeWindow_Buffer* out_buffer) {
        std::unique_lock lk(mSurfaceMutex);

        Surface* surface = mNativeSurface.get();
        if (surface == nullptr) {
            // Surface not currently available but not necessarily an error
            // if, for example, the VmLauncherApp is not in the foreground.
            *out_buffer = mSinkBuffer;
            return {};
        }

        ANativeWindow* anw = surface->get();
        if (anw == nullptr) {
            return Error() << "Failed to get ANativeWindow";
        }

        if (auto ret = prepareNativeWindowLocked(anw, false); !ret.ok()) {
            return ret;
        }

        if (ANativeWindow_lock(anw, out_buffer, nullptr) != 0) {
            return Error() << "Failed to lock window";
        }
        mLastBuffer = *out_buffer;
        mLastBufferValid = true;
        return {};
    }

    Result<int> flip(VulkanDisplayBridge& bridge, int64_t importId) {
        ScopedTrace trace("crosvm_display.flip");
        std::unique_lock lk(mSurfaceMutex);

        Surface* surface = mNativeSurface.get();
        if (surface == nullptr) {
            // The display service can temporarily lose its Surface while the launcher is
            // backgrounded. Keep the source import alive and drop the frame in that interval.
            return -1;
        }
        if (mVulkanTargetGeneration != mNativeSurfaceGeneration) {
            if (auto ret = bridge.resetTargetsForSurfaceChange(); !ret.ok()) {
                return Error() << ret.error();
            }
            mVulkanTargetGeneration = mNativeSurfaceGeneration;
        }
        ANativeWindow* anw = surface->get();
        if (anw == nullptr) return Error() << "Failed to get ANativeWindow";
        {
            ScopedTrace prepareTrace("crosvm_display.prepare_window");
            if (auto ret = prepareNativeWindowLocked(anw, true); !ret.ok()) {
                return Error() << ret.error();
            }
        }

        ANativeWindowBuffer* buffer = nullptr;
        int fenceFd = -1;
        int result;
        const bool allowFaultInjection = mName == "scanout";
        if (allowFaultInjection && injectRuntimeFlipFailure(RuntimeFlipFailureStage::kDequeue)) {
            return Error() << "Injected display dequeue failure";
        }
        {
            ScopedTrace dequeueTrace("crosvm_display.dequeue_buffer");
            result = ANativeWindow_dequeueBuffer(anw, &buffer, &fenceFd);
        }
        if (result == -ETIMEDOUT || result == -EWOULDBLOCK) {
            // Every buffer is queued to SurfaceFlinger and none has been released yet (the
            // producer runs with a zero dequeue timeout, see prepareNativeWindowLocked). Drop
            // this frame instead of stalling: nothing has touched the guest's source dmabuf, so
            // returning without a completion fence lets the guest reuse it right away, and the
            // next flush will present a newer frame.
            if (fenceFd >= 0) close(fenceFd);
            const uint64_t dropped = ++mDroppedFlips;
            if ((dropped & 1023) == 1) {
                LOG(INFO) << "display surface " << mName << " dropped " << dropped
                          << " flip(s) so far: no free display buffer (panel-paced)";
            }
            return -1;
        }
        if (result != 0) {
            if (fenceFd >= 0) close(fenceFd);
            if (clearIfSurfaceUnavailableLocked("dequeueBuffer", result)) return -1;
            return Error() << "Failed to dequeue display buffer: " << result;
        }
        if (buffer == nullptr) {
            if (fenceFd >= 0) close(fenceFd);
            return Error() << "Failed to dequeue display buffer: null buffer";
        }

        // When the bridge can import the acquire fence as a GPU wait semaphore, hand the fence to
        // blit() (which takes ownership) instead of blocking the display thread on a CPU poll.
        int acquireFenceFd = -1;
        if (bridge.canImportAcquireFence()) {
            acquireFenceFd = fenceFd;
            fenceFd = -1;
        } else if (auto ret = waitForFence(fenceFd); !ret.ok()) {
            fenceFd = -1; // waitForFence closed it
            ANativeWindow_cancelBuffer(anw, buffer, -1);
            return Error() << ret.error();
        } else {
            fenceFd = -1; // waitForFence closed it
        }

        AHardwareBuffer* targetAhb = ANativeWindowBuffer_getHardwareBuffer(buffer);
        if (targetAhb == nullptr) {
            if (acquireFenceFd >= 0) {
                if (auto ret = waitForFence(acquireFenceFd); !ret.ok()) {
                    ANativeWindow_cancelBuffer(anw, buffer, -1);
                    return ret.error();
                }
            }
            ANativeWindow_cancelBuffer(anw, buffer, -1);
            return Error() << "Failed to get display target AHardwareBuffer";
        }
        auto completion = bridge.blit(importId, targetAhb, acquireFenceFd, allowFaultInjection);
        if (!completion.ok()) {
            // blit() owns acquireFenceFd on every return path, so do not close it here.
            ANativeWindow_cancelBuffer(anw, buffer, -1);
            return Error() << "Failed to blit into dequeued display buffer: " << completion.error();
        }
        int targetCompletionFd = *completion;
        int sourceCompletionFd = -1;
        if (targetCompletionFd >= 0) {
            sourceCompletionFd = dup(targetCompletionFd);
            if (sourceCompletionFd < 0) {
                PLOG(WARNING) << "Failed to duplicate display completion sync_fd; draining";
                if (auto ret = waitForFence(targetCompletionFd); !ret.ok()) {
                    ANativeWindow_cancelBuffer(anw, buffer, -1);
                    return ret.error();
                }
                targetCompletionFd = -1;
            }
        }
        const bool injectQueueFailure =
                allowFaultInjection && injectRuntimeFlipFailure(RuntimeFlipFailureStage::kQueue);
        bool queueCalled = false;
        if (injectQueueFailure) {
            result = -EIO;
        } else {
            ScopedTrace queueTrace("crosvm_display.queue_buffer");
            result = ANativeWindow_queueBuffer(anw, buffer, targetCompletionFd);
            queueCalled = true;
        }
        if (result != 0) {
            // The duplicate guarantees the target is no longer being written before it is
            // cancelled back to BufferQueue.
            if (sourceCompletionFd >= 0) {
                auto waitResult = waitForFence(sourceCompletionFd);
                sourceCompletionFd = -1;
                if (!waitResult.ok()) {
                    if (!queueCalled && targetCompletionFd >= 0) close(targetCompletionFd);
                    ANativeWindow_cancelBuffer(anw, buffer, -1);
                    return waitResult.error();
                }
            }
            // A real queueBuffer call consumes the completion fd even on error. The injected
            // failure skips that call, so close our still-owned fd after its duplicate signals.
            if (!queueCalled && targetCompletionFd >= 0) close(targetCompletionFd);
            ANativeWindow_cancelBuffer(anw, buffer, -1);
            if (!injectQueueFailure && clearIfSurfaceUnavailableLocked("queueBuffer", result)) {
                return -1;
            }
            return Error() << (injectQueueFailure ? "Injected display queue failure: "
                                                  : "Failed to queue display buffer: ")
                           << result;
        }
        return sourceCompletionFd;
    }

    Result<void> unlockAndPost() {
        std::unique_lock lk(mSurfaceMutex);

        Surface* surface = mNativeSurface.get();
        if (surface == nullptr) {
            // Surface not currently available but not necessarily an error
            // if, for example, the VmLauncherApp is not in the foreground.
            return {};
        }

        ANativeWindow* anw = surface->get();
        if (anw == nullptr) {
            return Error() << "Failed to get ANativeWindow";
        }

        // The CPU edge has already copied into the RGBA layout declared by this sink, so this hash
        // is also exactly what SurfaceFlinger receives. See frameHashEnabled().
        if (frameHashEnabled()) logFrameHash(mName, mLastBuffer);

        if (ANativeWindow_unlockAndPost(anw) != 0) {
            return Error() << "Failed to unlock and post window";
        }
        return {};
    }

    Result<void> setBuffer(AHardwareBuffer* ahb) {
        std::lock_guard lk(mSurfaceMutex);
        auto& sc = SurfaceControl::GetInstance();
        if (!sc.IsSupported()) {
            return Error() << "SurfaceControl is not supported";
        }
        auto transaction = sc.ASurfaceTransaction_create();
        if (!transaction) {
            return Error() << "Failed to create ASurfaceTransaction";
        }
        if (!mSurfaceControl) {
            return Error() << "mSurfaceControl is destroyed";
        }
        sc.ASurfaceTransaction_setBuffer(transaction, mSurfaceControl, ahb,
                                         -1 /* acquire_fence_fd */);
        sc.ASurfaceTransaction_apply(transaction);
        sc.ASurfaceTransaction_delete(transaction);
        return {};
    }

    // Saves the last frame drawn
    Result<void> saveFrame() {
        std::unique_lock lk(mSurfaceMutex);
        if (!mLastBufferValid) return {};
        if (auto ret = copyBuffer(mLastBuffer, mSavedFrameBuffer); !ret.ok()) {
            return Error() << "Failed to copy frame: " << ret.error();
        }
        mSavedFrameValid = true;
        return {};
    }

    // Draws the saved frame
    Result<void> drawSavedFrame() {
        std::unique_lock lk(mSurfaceMutex);
        if (!mSavedFrameValid) return {};
        Surface* surface = mNativeSurface.get();
        if (surface == nullptr) {
            return Error() << "Surface not ready";
        }

        ANativeWindow* anw = surface->get();
        if (anw == nullptr) {
            return Error() << "Failed to get ANativeWindow";
        }

        if (auto ret = prepareNativeWindowLocked(anw, false); !ret.ok()) return ret;

        ANativeWindow_Buffer buf;
        if (ANativeWindow_lock(anw, &buf, nullptr) != 0) {
            return Error() << "Failed to lock window";
        }

        if (auto ret = copyBuffer(mSavedFrameBuffer, buf); !ret.ok()) {
            return Error() << "Failed to copy frame: " << ret.error();
        }

        if (ANativeWindow_unlockAndPost(anw) != 0) {
            return Error() << "Failed to unlock and post window";
        }
        return {};
    }

    const std::string& name() const { return mName; }

private:
    void clearNativeSurfaceLocked(bool disconnect) {
        const bool hadSurface = mNativeSurface != nullptr;
        if (disconnect) {
            disconnectNativeWindowLocked();
        } else {
            mConnectedNativeWindowApi = 0;
            mGpuUsageConfigured = false;
            mZeroDequeueTimeout = false;
        }

        auto& sc = SurfaceControl::GetInstance();
        if (mSurfaceControl) {
            if (sc.IsSupported()) sc.ASurfaceControl_release(mSurfaceControl);
            mSurfaceControl = nullptr;
        }
        mNativeSurface = nullptr;
        mNativeSurfaceNeedsConfiguring = true;
        mGpuUsageConfigured = false;
        mLastBufferValid = false;
        if (hadSurface) ++mNativeSurfaceGeneration;
    }

    bool clearIfSurfaceUnavailableLocked(const char* operation, int status) {
        if (!isSurfaceUnavailableStatus(status)) return false;
        LOG(WARNING) << "display surface " << mName << " became unavailable during " << operation
                     << " (" << status
                     << "); keeping Vulkan path active and dropping frames until a replacement is "
                        "attached";
        // The producer is already disconnected or abandoned. Avoid another native-window call
        // while releasing our stale Surface reference.
        clearNativeSurfaceLocked(false);
        return true;
    }

    void disconnectNativeWindowLocked() {
        if (mConnectedNativeWindowApi == 0) return;
        Surface* surface = mNativeSurface.get();
        ANativeWindow* anw = surface ? surface->get() : nullptr;
        if (anw) {
            int result = native_window_api_disconnect(anw, mConnectedNativeWindowApi);
            if (result != 0) {
                LOG(WARNING) << "display surface " << mName
                             << " failed to disconnect native window API "
                             << mConnectedNativeWindowApi << ": " << result;
            }
        }
        mConnectedNativeWindowApi = 0;
        mGpuUsageConfigured = false;
        mZeroDequeueTimeout = false;
    }

    static Result<void> waitForFence(int fenceFd) {
        ScopedTrace trace("crosvm_display.target_fence_wait");
        if (fenceFd < 0) return {};
        pollfd descriptor{
                .fd = fenceFd,
                .events = POLLIN,
                .revents = 0,
        };
        int result;
        do {
            result = poll(&descriptor, 1, -1);
        } while (result < 0 && errno == EINTR);
        close(fenceFd);
        if (result <= 0) return Error() << "Failed to wait for display acquire fence";
        return {};
    }

    Result<void> prepareNativeWindowLocked(ANativeWindow* anw, bool gpu) {
        const int requestedApi = gpu ? NATIVE_WINDOW_API_EGL : NATIVE_WINDOW_API_CPU;
        if (mConnectedNativeWindowApi != 0 && mConnectedNativeWindowApi != requestedApi) {
            int result = native_window_api_disconnect(anw, mConnectedNativeWindowApi);
            if (result != 0) {
                return Error() << "Failed to switch native window producer API: " << result;
            }
            mConnectedNativeWindowApi = 0;
            mGpuUsageConfigured = false;
            mNativeSurfaceNeedsConfiguring = true;
        }
        if (mConnectedNativeWindowApi == 0) {
            int result = native_window_api_connect(anw, requestedApi);
            if (result != 0) return Error() << "Failed to connect native window: " << result;
            mConnectedNativeWindowApi = requestedApi;
        }

        // The GPU (blit) producer must never block in dequeueBuffer. With the default timeout
        // (-1) a flip stalls until SurfaceFlinger releases a buffer at its next vsync, and since
        // the guest's RESOURCE_FLUSH fence is deferred to this blit's completion, the whole guest
        // compositor ends up paced by the phone panel: on a 60 Hz ColorOS panel venus/drm2kgsl
        // ran at 70-800 fps with the display attached and 4000-9000 fps without it. A timeout of
        // 0 makes dequeueBuffer return TIMED_OUT when every buffer is queued; flip() drops that
        // frame and the guest keeps its own pace, the display shows the newest frame it can.
        // The CPU-copy producer keeps the blocking default so ANativeWindow_lock behaves as before.
        const bool wantZeroTimeout = gpu;
        if (mZeroDequeueTimeout != wantZeroTimeout) {
            const int64_t timeout = wantZeroTimeout ? 0 : -1;
            int result = anw->perform(anw, NATIVE_WINDOW_SET_DEQUEUE_TIMEOUT, timeout);
            if (result != 0) {
                LOG(WARNING) << "display surface " << mName << " set dequeue timeout " << timeout
                             << " failed: " << result;
            } else {
                mZeroDequeueTimeout = wantZeroTimeout;
            }
        }

        if (!mRequestedSurfaceDimensions) {
            return Error() << "Surface dimension is not configured yet!";
        }
        const auto& dims = *mRequestedSurfaceDimensions;
        if (mNativeSurfaceNeedsConfiguring) {
            LOG(INFO) << "display surface " << mName << " set buffer geometry " << dims.width << "x"
                      << dims.height;
            if (ANativeWindow_setBuffersGeometry(anw, dims.width, dims.height, kFormat) != 0) {
                return Error() << "Failed to set buffer geometry.";
            }
            mNativeSurfaceNeedsConfiguring = false;
        }

        if (gpu && !mGpuUsageConfigured) {
            constexpr uint64_t kGpuUsage = AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE |
                    AHARDWAREBUFFER_USAGE_GPU_FRAMEBUFFER | AHARDWAREBUFFER_USAGE_COMPOSER_OVERLAY;
            if (ANativeWindow_setUsage(anw, kGpuUsage) != 0) {
                return Error() << "Failed to set GPU buffer usage";
            }
            mGpuUsageConfigured = true;
        } else if (!gpu) {
            mGpuUsageConfigured = false;
        }
        return {};
    }

    // Keep the Android buffer RGBA_8888 (not BGRA_8888) so it imports as a regular GL_TEXTURE_2D.
    // A BGRA buffer can become GL_TEXTURE_EXTERNAL_OES on some Adreno devices, whose Skia
    // RenderEngine then aborts in makeImage ("Unable to generate SkImage"), crashing
    // surfaceflinger. crosvm now declares this RGBA layout as the CPU sink format and performs any
    // required R/B exchange while copying into the buffer; no post-process is needed here. RGBA
    // also preserves the alpha channel used for cursor blending.
    static constexpr const int kFormat = HAL_PIXEL_FORMAT_RGBA_8888;

    std::string mName;

    std::mutex mSurfaceMutex;
    std::unique_ptr<Surface> mNativeSurface;
    ASurfaceControl* mSurfaceControl = nullptr;
    std::condition_variable mNativeSurfaceReady;
    bool mNativeSurfaceNeedsConfiguring = true;
    int mConnectedNativeWindowApi = 0;
    bool mGpuUsageConfigured = false;
    // Whether the connected producer currently runs with a zero dequeue timeout (GPU path).
    bool mZeroDequeueTimeout = false;
    // Flips dropped because no display buffer was free at the time (see flip()).
    uint64_t mDroppedFlips = 0;
    uint64_t mNativeSurfaceGeneration = 0;
    uint64_t mVulkanTargetGeneration = 0;

    // Buffer which crosvm uses when in background. This is just to not fail crosvm even when
    // Android-side Surface doesn't exist. The content drawn here is never displayed on the physical
    // screen.
    SinkANativeWindow_Buffer mSinkBuffer;

    // Buffer which is currently allocated for crosvm to draw onto. This holds the last frame. This
    // is what gets displayed on the physical screen.
    ANativeWindow_Buffer mLastBuffer;
    bool mLastBufferValid = false;

    // Copy of mLastBuffer made by the call saveFrameForSurface. This holds the last good (i.e.
    // non-blank) frame before the VM goes background. When the VM is brought up to foreground,
    // this is drawn to the physical screen until the VM starts to emit actual frames.
    SinkANativeWindow_Buffer mSavedFrameBuffer;
    bool mSavedFrameValid = false;

    struct Rect {
        uint32_t width = 0;
        uint32_t height = 0;
    };
    std::optional<Rect> mRequestedSurfaceDimensions;

public:
    // Current scanout size (0x0 until the first configure()). Read by getDisplayConfig so the app
    // can follow guest resolution changes; configure() is the single writer for every cause.
    Rect dimensions() {
        std::lock_guard lk(mSurfaceMutex);
        return mRequestedSurfaceDimensions.value_or(Rect{});
    }
};

class DisplayService : public BnCrosvmAndroidDisplayService {
public:
    DisplayService() = default;
    virtual ~DisplayService() = default;

    ndk::ScopedAStatus setSurface(Surface* surface, bool forCursor) override {
        getSurface(forCursor).setNativeSurface(surface);
        return ::ndk::ScopedAStatus::ok();
    }

    ndk::ScopedAStatus removeSurface(bool forCursor) override {
        getSurface(forCursor).removeSurface();
        return ::ndk::ScopedAStatus::ok();
    }

    ndk::ScopedFileDescriptor& getCursorStream() { return mCursorStream; }
    ndk::ScopedAStatus setCursorStream(const ndk::ScopedFileDescriptor& in_stream) {
        mCursorStream = ndk::ScopedFileDescriptor(dup(in_stream.get()));
        return ::ndk::ScopedAStatus::ok();
    }

    ndk::ScopedAStatus saveFrameForSurface(bool forCursor) override {
        if (auto ret = getSurface(forCursor).saveFrame(); !ret.ok()) {
            std::string msg = std::format("Failed to save frame: {}", ret.error().message());
            return ::ndk::ScopedAStatus(
                    AStatus_fromServiceSpecificErrorWithMessage(-1, msg.c_str()));
        }
        return ::ndk::ScopedAStatus::ok();
    }

    ndk::ScopedAStatus drawSavedFrameForSurface(bool forCursor) override {
        if (auto ret = getSurface(forCursor).drawSavedFrame(); !ret.ok()) {
            std::string msg = std::format("Failed to draw saved frame: {}", ret.error().message());
            return ::ndk::ScopedAStatus(
                    AStatus_fromServiceSpecificErrorWithMessage(-1, msg.c_str()));
        }
        return ::ndk::ScopedAStatus::ok();
    }

    ndk::ScopedAStatus getDisplayConfig(
            ::aidl::android::crosvm::DisplayConfig* _aidl_return) override {
        auto dims = mScanout.dimensions();
        _aidl_return->width = static_cast<int32_t>(dims.width);
        _aidl_return->height = static_cast<int32_t>(dims.height);
        // dpi/refreshRate aren't tracked here; the app only consumes width/height for letterboxing.
        _aidl_return->dpi = 0;
        _aidl_return->refreshRate = 0;
        return ::ndk::ScopedAStatus::ok();
    }

    AndroidDisplaySurface& getSurface(bool forCursor) {
        if (forCursor) {
            return mCursor;
        } else {
            return mScanout;
        }
    }

private:
    AndroidDisplaySurface mScanout{"scanout"};
    AndroidDisplaySurface mCursor{"cursor"};
    ndk::ScopedFileDescriptor mCursorStream;
};

} // namespace

typedef void (*ErrorCallback)(const char* message);

struct AndroidDisplayContext {
    enum class DisplayPath {
        kUnknown,
        kVulkanBlit,
        kCpu,
    };

    std::shared_ptr<DisplayService> disp_service;
    ErrorCallback error_callback;
    DisplayPath display_path = DisplayPath::kUnknown;
    std::unique_ptr<VulkanDisplayBridge> vulkan_display;

    AndroidDisplayContext(const char* service_name, ErrorCallback cb) : error_callback(cb) {
        if (!envFlagEnabled("CROSVM_ANDROID_DISPLAY_VULKAN_BLIT", true)) {
            display_path = DisplayPath::kCpu;
            LOG(INFO) << "Android display path forced to CPU by feature flag";
        } else {
            LOG(INFO) << "Android display path is unprobed";
        }
        auto disp_service = ::ndk::SharedRefBase::make<DisplayService>();

        // Register the DisplayService directly to the service manager under `service_name`
        // (passed from crosvm's --android-display-service argument). This works because crosvm
        // here is launched in a privileged (root) context with SELinux permissive, so it is
        // allowed to register a service. The app side looks the service up by the same name.
        //
        // The binder thread pool MUST be started before addService so that incoming calls
        // (e.g. setSurface) from the app are serviced.
        ABinderProcess_setThreadPoolMaxThreadCount(4);
        ABinderProcess_startThreadPool();

        binder_status_t status =
                AServiceManager_addService(disp_service->asBinder().get(), service_name);
        if (status != STATUS_OK) {
            errorf("Failed to register '%s' to service manager: status=%d", service_name, status);
            return;
        }

        this->disp_service = disp_service;
    }

    ~AndroidDisplayContext() = default;

    VulkanDisplayBridge* getVulkanDisplay() {
        if (display_path == DisplayPath::kCpu) return nullptr;
        if (display_path == DisplayPath::kUnknown) {
            vulkan_display = std::make_unique<VulkanDisplayBridge>();
            if (!vulkan_display->ready()) {
                display_path = DisplayPath::kCpu;
                LOG(WARNING) << "Android display M1 probe failed; using CPU for this process";
                return nullptr;
            }
            display_path = DisplayPath::kVulkanBlit;
            LOG(INFO) << "Android display M1 probe succeeded; using Vulkan blit";
        }
        return vulkan_display.get();
    }

    bool vulkanDisplayActive() const { return display_path == DisplayPath::kVulkanBlit; }

    void fallbackToCpu(const char* reason) {
        if (display_path == DisplayPath::kCpu) return;
        display_path = DisplayPath::kCpu;
        LOG(WARNING) << "Android display falling back to CPU for this process: " << reason;
        // Rust resource states release imports lazily as those resources are revisited. Destroy
        // the bridge now so inactive source imports, cached AHB targets, and in-flight sync
        // objects cannot survive for the rest of a sticky CPU-fallback process.
        vulkan_display.reset();
    }

    void errorf(const char* format, ...) {
        char buffer[1024];

        va_list vararg;
        va_start(vararg, format);
        vsnprintf(buffer, sizeof(buffer), format, vararg);
        va_end(vararg);

        error_callback(buffer);
    }
};

extern "C" struct AndroidDisplayContext* create_android_display_context(
        const char* name, ErrorCallback error_callback) {
    return new AndroidDisplayContext(name, error_callback);
}

extern "C" void destroy_android_display_context(struct AndroidDisplayContext* ctx) {
    delete ctx;
}

extern "C" AndroidDisplaySurface* create_android_surface(struct AndroidDisplayContext* ctx,
                                                         uint32_t width, uint32_t height,
                                                         bool forCursor) {
    if (ctx->disp_service == nullptr) {
        ctx->errorf("Display service was not created");
        return nullptr;
    }

    AndroidDisplaySurface& surface = ctx->disp_service->getSurface(forCursor);
    if (auto ret = surface.configure(width, height); !ret.ok()) {
        ctx->errorf("Failed to configure surface %s: %s", surface.name().c_str(),
                    ret.error().message().c_str());
    }

    // TODO(b/332785161): if we know that surface can get destroyed dynamically while VM is running,
    // consider calling ANativeWindow_acquire here and _release in destroy_android_surface, so that
    // crosvm doesn't hold a dangling pointer.
    return &surface;
}

extern "C" void destroy_android_surface(struct AndroidDisplayContext*, ANativeWindow*) {
    // NOT IMPLEMENTED
}

extern "C" bool get_android_surface_buffer(struct AndroidDisplayContext* ctx,
                                           AndroidDisplaySurface* surface,
                                           ANativeWindow_Buffer* out_buffer) {
    if (out_buffer == nullptr) {
        ctx->errorf("out_buffer is null");
        return false;
    }

    if (surface == nullptr) {
        ctx->errorf("Invalid AndroidDisplaySurface provided");
        return false;
    }

    auto ret = surface->lock(out_buffer);
    if (!ret.ok()) {
        ctx->errorf("Failed to lock surface %s: %s", surface->name().c_str(),
                    ret.error().message().c_str());
        return false;
    }

    return true;
}

extern "C" void set_android_surface_position(struct AndroidDisplayContext* ctx, uint32_t x,
                                             uint32_t y) {
    if (ctx->disp_service == nullptr) {
        ctx->errorf("Display service was not created");
        return;
    }
    auto fd = ctx->disp_service->getCursorStream().get();
    if (fd == -1) {
        static std::atomic<bool> warned = false;
        if (!warned.exchange(true, std::memory_order_relaxed)) {
            LOG(WARNING) << "cursor position stream is not attached; dropping position updates";
        }
        return;
    }
    uint32_t pos[] = {x, y};
    write(fd, pos, sizeof(pos));
}

extern "C" void post_android_surface_buffer(struct AndroidDisplayContext* ctx,
                                            AndroidDisplaySurface* surface) {
    if (surface == nullptr) {
        ctx->errorf("Invalid AndroidDisplaySurface provided");
        return;
    }

    auto ret = surface->unlockAndPost();
    if (!ret.ok()) {
        ctx->errorf("Failed to unlock and post for surface %s: %s", surface->name().c_str(),
                    ret.error().message().c_str());
    }
    return;
}

extern "C" void set_android_surface_buffer_format(struct AndroidDisplayContext* ctx,
                                                  AndroidDisplaySurface* surface, uint32_t fourcc) {
    if (ctx == nullptr || surface == nullptr) {
        return;
    }
    // The app-side ANativeWindow is configured RGBA_8888 only (see configure()); until buffer
    // reallocation plumbing exists app-side, the guest's scanout fourcc is diagnostic only.
    static std::atomic<uint32_t> lastFourcc{0};
    if (lastFourcc.exchange(fourcc, std::memory_order_relaxed) != fourcc) {
        LOG(INFO) << "CROSVM_DISPLAY_FOURCC 0x" << std::hex << fourcc << std::dec
                  << " for surface " << surface->name() << " (format switch not implemented)";
    }
}

extern "C" int64_t android_display_import_dmabuf(struct AndroidDisplayContext* ctx,
                                                 AndroidDisplaySurface* surface, int fd,
                                                 uint32_t offset, uint32_t stride,
                                                 uint64_t modifier, bool linearLayoutVerified,
                                                 uint32_t width, uint32_t height, uint32_t fourcc) {
    if (!ctx || !surface) return 0;
    static bool loggedImportLayout = false;
    if (!loggedImportLayout) {
        loggedImportLayout = true;
        LOG(INFO) << "CROSVM_DISPLAY_IMPORT fd=" << fd << " offset=0x" << std::hex << offset
                  << " stride=0x" << stride << " modifier=0x" << modifier << " fourcc=0x" << fourcc
                  << std::dec << " linear_verified=" << linearLayoutVerified << " size=" << width
                  << "x" << height;
    }
    VulkanDisplayBridge* bridge = ctx->getVulkanDisplay();
    if (!bridge) return 0;
    int64_t importId = bridge->importDmabuf(fd, offset, stride, modifier, linearLayoutVerified,
                                            width, height, fourcc);
    if (!importId) {
        ctx->errorf("Failed to import display dmabuf (%ux%u stride=%u fourcc=0x%x)", width, height,
                    stride, fourcc);
        return 0;
    }
    return importId;
}

extern "C" bool android_display_is_vulkan_blit_available(struct AndroidDisplayContext* ctx) {
    return ctx && ctx->getVulkanDisplay() != nullptr;
}

// Whether a frame posted now can reach a screen: the app attached a Surface to the scanout and has
// not taken it back. Leaving the display view destroys the SurfaceView, and the removeSurface(false)
// that follows drops the native window here; the same clearing happens when a producer is abandoned
// under us (clearIfSurfaceUnavailableLocked). While there is none, lock() hands out the sink buffer
// and unlockAndPost() returns without posting, so the frame is built and thrown away.
//
// Only the scanout counts. The cursor surface is a separate SurfaceView on its own lifecycle and may
// never be attached at all (the app's display view does not always carry a cursor overlay), so it
// answers a different question and would make this one wrong in both directions.
//
// A caller is entitled to skip building the frame on a false, so this must never block or wait: the
// answer is the current state, and a consumer that arrives a moment later is seen by the next call.
extern "C" bool android_display_has_consumer(struct AndroidDisplayContext* ctx) {
    if (ctx == nullptr || ctx->disp_service == nullptr) return false;
    return ctx->disp_service->getSurface(/* forCursor= */ false).hasNativeSurface();
}

extern "C" void android_display_release_import(struct AndroidDisplayContext* ctx,
                                               int64_t rawHandle) {
    if (!ctx || !ctx->vulkan_display || !rawHandle) return;
    ctx->vulkan_display->release(rawHandle);
}

extern "C" bool android_display_flip_to(struct AndroidDisplayContext* ctx,
                                        AndroidDisplaySurface* surface, int64_t rawHandle,
                                        int* outCompletionFenceFd) {
    if (outCompletionFenceFd) *outCompletionFenceFd = -1;
    if (!ctx || !surface || !rawHandle || !outCompletionFenceFd || !ctx->vulkan_display ||
        !ctx->vulkanDisplayActive()) {
        return false;
    }
    auto ret = surface->flip(*ctx->vulkan_display, rawHandle);
    if (!ret.ok()) {
        ctx->errorf("Failed to flip imported buffer for surface %s: %s", surface->name().c_str(),
                    ret.error().message().c_str());
        ctx->fallbackToCpu("M1 runtime flip failed");
        return false;
    }
    *outCompletionFenceFd = *ret;
    return true;
}

// ---------------------------------------------------------------------------------------------
// The headless blit: the same Vulkan machinery, with a buffer we can read instead of a screen.
//
// Separate entry points rather than a flag on the display context above, because there is no
// display: no service name, no binder registration, no Surface, no cursor. A caller that wants this
// wants only the blit, and giving it the display context would mean it had to be told which half of
// that object was real.
// ---------------------------------------------------------------------------------------------

struct AndroidBlitContext {
    HeadlessBlitContext impl;
    AndroidBlitContext(uint32_t width, uint32_t height) : impl(width, height) {}
};

// Creates a blit context, or returns null if this process has no Vulkan blit driver to load. Null
// is a normal answer, not an error: it is what a machine with no CROSVM_DISPLAY_VULKAN_LIBRARY
// named says, and every caller of this already has a CPU path to fall to.
extern "C" struct AndroidBlitContext* android_blit_ctx_create(uint32_t width, uint32_t height) {
    auto ctx = std::make_unique<AndroidBlitContext>(width, height);
    if (!ctx->impl.ready()) {
        LOG(INFO) << "headless blit context unavailable; caller stays on the CPU copy";
        return nullptr;
    }
    LOG(INFO) << "headless blit context ready for " << width << "x" << height;
    return ctx.release();
}

extern "C" void android_blit_ctx_destroy(struct AndroidBlitContext* ctx) {
    delete ctx;
}

// Imports a guest dmabuf as a blit source. Returns 0 if it cannot be imported. `fourcc` is the
// GUEST's declaration; what the source image is actually created with is HeadlessBlitContext's
// business (see blitSourceFourcc).
//
// `exchange_red_blue` says which byte order blits from this import must land in: true for the
// CPU pipeline's canonical B,G,R,X (what LibVNCServer serves), false for the R,G,B,A an
// RGBA_8888 gralloc buffer claims to hold (what a video encoder reads). One dmabuf can be
// imported both ways at once; each import is one VkImage over the same guest pages.
extern "C" int64_t android_blit_ctx_import_dmabuf(struct AndroidBlitContext* ctx, int fd,
                                                  uint32_t offset, uint32_t stride,
                                                  uint64_t modifier, bool linearLayoutVerified,
                                                  uint32_t width, uint32_t height, uint32_t fourcc,
                                                  bool exchange_red_blue) {
    if (!ctx) return 0;
    return ctx->impl.importDmabuf(fd, offset, stride, modifier, linearLayoutVerified, width, height,
                                  fourcc, exchange_red_blue);
}

extern "C" void android_blit_ctx_release_import(struct AndroidBlitContext* ctx, int64_t importId) {
    if (!ctx || !importId) return;
    ctx->impl.release(importId);
}

// Blits an import into the context's target and waits, with a bound, for the GPU to finish. False
// means the frame did not happen and the caller should fall back.
extern "C" bool android_blit_ctx_blit(struct AndroidBlitContext* ctx, int64_t importId,
                                      uint32_t width, uint32_t height, int timeout_ms) {
    if (!ctx || !importId) return false;
    auto ret = ctx->impl.blit(importId, width, height, timeout_ms);
    if (!ret.ok()) {
        LOG(WARNING) << "headless blit failed: " << ret.error().message();
        return false;
    }
    return true;
}

// Maps the target for CPU reading. The mapping is valid until the next android_blit_ctx_blit or
// android_blit_ctx_unmap on this context, or until it is destroyed. Out params are only written on
// success.
extern "C" bool android_blit_ctx_map(struct AndroidBlitContext* ctx, const uint8_t** out_pixels,
                                     uint32_t* out_stride_bytes, uint32_t* out_width,
                                     uint32_t* out_height, uint32_t* out_size) {
    if (!ctx || !out_pixels || !out_stride_bytes || !out_width || !out_height || !out_size) {
        return false;
    }
    auto ret = ctx->impl.map(out_pixels, out_stride_bytes, out_width, out_height, out_size);
    if (!ret.ok()) {
        LOG(WARNING) << "headless blit map failed: " << ret.error().message();
        return false;
    }
    return true;
}

extern "C" void android_blit_ctx_unmap(struct AndroidBlitContext* ctx) {
    if (!ctx) return;
    ctx->impl.unmap();
}

// ---------------------------------------------------------------------------------------------
// The H.264 side channel's encoder half (plan §6 step 13).
//
// Deliberately shaped like the blit entry points above rather than folded into them: a caller can
// have a blit context and no encoder (every VNC screen today) or, in principle, an encoder and no
// blit context (a CPU-transport producer whose frames are uploaded). The one place the two meet is
// android_h264_enc_encode_frame, which takes the blit context as an argument -- so the coupling is
// per frame and visible in the signature, instead of a field somebody has to remember to set.
//
// Everything above the socket is here; nothing about the socket is. Which port the stream leaves
// by, who is listening on it and what the framing looks like are crosvm's, and this side never
// learns any of it.
// ---------------------------------------------------------------------------------------------

struct AndroidH264Encoder {
    std::unique_ptr<H264EncoderSession> impl;
};

// Brings up a hardware H.264 encoder for a screen of this size, or returns null.
//
// Null is an ordinary answer with several ordinary causes -- no media NDK, no AVC encoder, a
// geometry the component refuses -- and the caller's response to all of them is the same: serve
// RFB and nothing else. The reason is logged here, once, because here is where it is known.
extern "C" struct AndroidH264Encoder* android_h264_enc_create(uint32_t width, uint32_t height,
                                                              int32_t bitrate_bps,
                                                              int32_t frame_rate,
                                                              int32_t iframe_interval_secs) {
    auto session = H264EncoderSession::Open(width, height, bitrate_bps, frame_rate,
                                            iframe_interval_secs);
    if (!session) return nullptr;
    auto ctx = std::make_unique<AndroidH264Encoder>();
    ctx->impl = std::move(session);
    return ctx.release();
}

extern "C" void android_h264_enc_destroy(struct AndroidH264Encoder* ctx) {
    delete ctx;
}

// Asks for an IDR at the next opportunity. Called when a side-channel client connects.
extern "C" void android_h264_enc_request_sync_frame(struct AndroidH264Encoder* ctx) {
    if (!ctx) return;
    ctx->impl->requestSyncFrame();
}

// Feeds one picture. `import_id` non-zero (with `blit_ctx`) takes the GPU route -- the guest's own
// dmabuf blitted into the codec's input buffer -- and zero takes the CPU upload of `pixels`.
//
// False means the frame did not reach the encoder. `out_error` receives a NUL-terminated reason
// when it did not, so the caller can report the first failure verbatim rather than "hw encode did
// not work": on the GPU route that message IS the §7 verdict.
extern "C" bool android_h264_enc_encode_frame(struct AndroidH264Encoder* ctx,
                                              struct AndroidBlitContext* blit_ctx,
                                              int64_t import_id, const uint8_t* pixels,
                                              uint32_t pixels_size, uint32_t width, uint32_t height,
                                              const uint8_t* cursor_bgra, int32_t cursor_w,
                                              int32_t cursor_h, int32_t cursor_x, int32_t cursor_y,
                                              bool cursor_visible, int64_t pts_us, char* out_error,
                                              uint32_t error_cap) {
    if (out_error && error_cap) out_error[0] = '\0';
    if (!ctx) return false;
    H264CursorOverlay cursor{
            .pixels = cursor_bgra,
            .width = cursor_w,
            .height = cursor_h,
            .x = cursor_x,
            .y = cursor_y,
            .visible = cursor_visible,
    };
    auto ret = ctx->impl->encodeFrame(blit_ctx ? &blit_ctx->impl : nullptr, import_id, pixels,
                                      pixels_size, width, height, cursor, pts_us);
    if (ret.ok()) return true;
    if (out_error && error_cap) {
        const std::string message = ret.error().message();
        const size_t copied = std::min<size_t>(message.size(), error_cap - 1);
        memcpy(out_error, message.data(), copied);
        out_error[copied] = '\0';
    }
    return false;
}

// Drains one compressed buffer, blocking for at most `timeout_us`.
//
// Returns bytes written, 0 for "nothing was ready", -1 for "it did not fit" (with *out_size set to
// what it needs; the frame is still queued), and -2 for a codec error.
extern "C" int32_t android_h264_enc_poll_output(struct AndroidH264Encoder* ctx, uint8_t* out,
                                                uint32_t cap, uint32_t* out_size,
                                                uint32_t* out_flags, int64_t* out_pts_us,
                                                int64_t timeout_us) {
    if (!ctx || !out) return H264EncoderSession::kOutputFailed;
    return ctx->impl->pollOutput(out, cap, out_size, out_flags, out_pts_us, timeout_us);
}

// Copies the cached SPS+PPS, and returns how many bytes they are. A return greater than `cap`
// means nothing was written; zero means the encoder has not emitted them yet.
extern "C" uint32_t android_h264_enc_codec_config(struct AndroidH264Encoder* ctx, uint8_t* out,
                                                  uint32_t cap) {
    if (!ctx || !out) return 0;
    return static_cast<uint32_t>(ctx->impl->codecConfig(out, cap));
}

// How many frames were handed to the encoder, and how many were dropped because every input
// buffer was still with it. Reported rather than logged per frame: the ratio is the only honest
// way to read the stream's frame rate against the producer's offer rate.
extern "C" void android_h264_enc_frame_counts(struct AndroidH264Encoder* ctx, uint64_t* out_queued,
                                              uint64_t* out_dropped) {
    if (!ctx) return;
    if (out_queued) *out_queued = ctx->impl->queuedFrames();
    if (out_dropped) *out_dropped = ctx->impl->droppedFrames();
}
