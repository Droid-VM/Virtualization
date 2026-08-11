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
        const char* configuredPath = std::getenv("CROSVM_TURNIP_LIBRARY");
        const char* paths[] = {
                configuredPath,
                "/data/local/tmp/turnip-v29/libvulkan_freedreno.so",
        };
        for (const char* path : paths) {
            if (!path || !*path) continue;
            mLibrary = dlopen(path, RTLD_NOW | RTLD_LOCAL);
            if (mLibrary) {
                LOG(INFO) << "Android display loading Turnip from " << path;
                break;
            }
        }
        if (!mLibrary) {
            LOG(ERROR) << "failed to load Turnip: " << dlerror();
            return false;
        }

        auto* module = static_cast<HwModule*>(dlsym(mLibrary, "HMI"));
        if (!module || !module->methods || !module->methods->open ||
            module->methods->open(module, "vk0", reinterpret_cast<void**>(&mHal)) != 0 || !mHal ||
            !mHal->create_instance || !mHal->get_instance_proc_addr) {
            LOG(ERROR) << "invalid Turnip hwvulkan HMI";
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
        VkImageCreateInfo imageInfo = {
                .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                .pNext = &modifierInfo,
                .imageType = VK_IMAGE_TYPE_2D,
                .format = format,
                .extent = {imported.width, imported.height, 1},
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

// crosvm always renders scanouts in B,G,R,X byte order (BGRA/BGRX — see SetScanoutBlob in
// crosvm's devices/src/virtio/gpu/mod.rs). We advertise the Android buffer as RGBA_8888 (see
// kFormat) instead of BGRA_8888 so the compositor imports it as a regular GL_TEXTURE_2D rather
// than GL_TEXTURE_EXTERNAL_OES: some GPUs' Skia RenderEngine (observed on Adreno) abort with
// "Unable to generate SkImage" when asked to sample an external-OES BGRA texture, which crashes
// surfaceflinger and soft-reboots the device. To keep colors correct under the RGBA layout we
// swap the R and B channels in place just before posting the buffer.
static void swapRedBlueInPlace(const ANativeWindow_Buffer& buf) {
    uint8_t* base = static_cast<uint8_t*>(buf.bits);
    if (base == nullptr) return;
    for (int32_t y = 0; y < buf.height; y++) {
        uint8_t* px = base + static_cast<size_t>(y) * static_cast<size_t>(buf.stride) * 4;
#if defined(__aarch64__)
        int32_t x = 0;
        // Process 16 interleaved RGBA pixels at once. The scanout is BGRA/BGRX,
        // so only the first and third byte lanes need to be exchanged.
        for (; x + 16 <= buf.width; x += 16, px += 64) {
            uint8x16x4_t channels = vld4q_u8(px);
            uint8x16_t tmp = channels.val[0];
            channels.val[0] = channels.val[2];
            channels.val[2] = tmp;
            vst4q_u8(px, channels);
        }
        for (; x < buf.width; x++, px += 4) {
#else
        for (int32_t x = 0; x < buf.width; x++, px += 4) {
#endif
            uint8_t t = px[0];
            px[0] = px[2];
            px[2] = t;
        }
    }
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

        // crosvm wrote B,G,R,X into the buffer; convert to R,G,B,X to match kFormat (RGBA_8888).
        swapRedBlueInPlace(mLastBuffer);

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

    // Note: crosvm always uses BGRA8888 or BGRX8888. See devices/src/virtio/gpu/mod.rs in
    // crosvm where the SetScanoutBlob command is handled. We advertise the buffer as RGBA_8888
    // (not BGRA_8888) so it imports as a regular GL_TEXTURE_2D — a BGRA buffer can be imported as
    // GL_TEXTURE_EXTERNAL_OES on some GPUs (Adreno) whose Skia RenderEngine then aborts in
    // makeImage ("Unable to generate SkImage"), crashing surfaceflinger. crosvm's B,G,R,X pixels
    // are converted to R,G,B,X by swapRedBlueInPlace() before each post. RGBA keeps the alpha
    // channel (used for cursor blending), matching the old BGRA behavior.
    static constexpr const int kFormat = HAL_PIXEL_FORMAT_RGBA_8888;

    std::string mName;

    std::mutex mSurfaceMutex;
    std::unique_ptr<Surface> mNativeSurface;
    ASurfaceControl* mSurfaceControl = nullptr;
    std::condition_variable mNativeSurfaceReady;
    bool mNativeSurfaceNeedsConfiguring = true;
    int mConnectedNativeWindowApi = 0;
    bool mGpuUsageConfigured = false;
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
