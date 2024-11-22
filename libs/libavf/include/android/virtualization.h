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
#pragma once

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

__BEGIN_DECLS

/**
 * Represents a handle on a virtual machine config.
 */
typedef struct AVirtualMachineConfig AVirtualMachineConfig;

/**
 * Create a new virtual machine config object with no properties.
 *
 * This only creates the raw config object. `name` and `kernel` must be set with
 * calls to {@link AVirtualMachineConfig_setName} and {@link AVirtualMachineConfig_setKernel}.
 * Other properties, set by {@link AVirtualMachineConfig_setMemoryMib},
 * {@link AVirtualMachineConfig_setInitRd}, {@link AVirtualMachineConfig_addDisk},
 * {@link AVirtualMachineConfig_setProtectedVm}, and {@link AVirtualMachineConfig_setBalloon}
 * are optional.
 *
 * The caller takes ownership of the returned config object, and is responsible for releasing it by
 * calling {@link AVirtualMachineConfig_free}.
 *
 * \return A new virtual machine raw config object.
 */
AVirtualMachineConfig* AVirtualMachineConfig_createRaw();

/**
 * Destroy a virtual machine config object.
 *
 * \param config a virtual machine config object.
 *
 * `AVirtualMachineConfig_free` does nothing if `config` is null. A destroyed config object must
 * not be reused.
 */
void AVirtualMachineConfig_free(AVirtualMachineConfig* config);

/**
 * Set a name of a virtual machine.
 *
 * \param config a virtual machine config object.
 * \param name a pointer to a null-terminated string for the name.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_setName(AVirtualMachineConfig* config, const char* name);

/**
 * Set an instance ID of a virtual machine.
 *
 * \param config a virtual machine config object.
 * \param instanceId a pointer to a 64-byte buffer for the instance ID.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_setInstanceId(AVirtualMachineConfig* config, const int8_t* instanceId);

/**
 * Set a kernel image of a virtual machine.
 *
 * \param config a virtual machine config object.
 * \param fd a readable file descriptor containing the kernel image, or -1 to unset. If successful,
 *   `AVirtualMachineConfig_setKernel` takes ownership of `fd`.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_setKernel(AVirtualMachineConfig* config, int fd);

/**
 * Set an init rd of a virtual machine.
 *
 * \param config a virtual machine config object.
 * \param fd a readable file descriptor containing the kernel image, or -1 to unset. If successful,
 *   `AVirtualMachineConfig_setInitRd` takes ownership of `fd`.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_setInitRd(AVirtualMachineConfig* config, int fd);

/**
 * Add a disk for a virtual machine.
 *
 * \param config a virtual machine config object.
 * \param fd a readable file descriptor containing the disk image. If successful,
 *   `AVirtualMachineConfig_addDisk` takes ownership of `fd`.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_addDisk(AVirtualMachineConfig* config, int fd);

/**
 * Set how much memory will be given to a virtual machine.
 *
 * \param config a virtual machine config object.
 * \param memoryMib the amount of RAM to give the virtual machine, in MiB. 0 or negative to use the
 *   default.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_setMemoryMib(AVirtualMachineConfig* config, int32_t memoryMib);

/**
 * NOT IMPLEMENTED.
 *
 * \return It returns -ENOTSUP.
 */
int AVirtualMachineConfig_setDeviceTreeOverlay(AVirtualMachineConfig* config, const char* path);

/**
 * Set whether a virtual machine is protected or not.
 *
 * \param config a virtual machine config object.
 * \param protectedVm whether the virtual machine should be protected.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_setProtectedVm(AVirtualMachineConfig* config, bool protectedVm);

/**
 * Set whether a virtual machine uses memory ballooning or not.
 *
 * \param config a virtual machine config object.
 * \param balloon whether the virtual machine should use memory ballooning.
 *
 * \return If successful, it returns 0. If `config` is invalid, it returns `-EINVAL`.
 */
int AVirtualMachineConfig_setBalloon(AVirtualMachineConfig* config, bool balloon);

/**
 * NOT IMPLEMENTED.
 *
 * \return It returns `-ENOTSUP`.
 */
int AVirtualMachineConfig_setHypervisorSpecificAuthMethod(AVirtualMachineConfig* config,
                                                          bool enable);

/**
 * NOT IMPLEMENTED.
 *
 * \return It returns `-ENOTSUP`.
 */
int AVirtualMachineConfig_addCustomMemoryBackingFile(AVirtualMachineConfig* config, int fd,
                                                     size_t rangeStart, size_t rangeEnd);

/**
 * NOT IMPLEMENTED.
 *
 * \return It returns `-ENOTSUP`.
 */
int AVirtualMachineConfig_addReservedMmioRange(AVirtualMachineConfig* config, size_t rangeStart,
                                               size_t rangeEnd);

/**
 * Represents a handle on a virtualization service, responsible for managing virtual machines.
 */
typedef struct AVirtualizationService AVirtualizationService;

/**
 * Spawn a new instance of `virtmgr`, a child process that will host the `VirtualizationService`
 * service, and connect to the child process.
 *
 * The caller takes ownership of the returned service object, and is responsible for releasing it
 * by calling {@link AVirtualizationService_free}.
 *
 * \param early set to true when running a service for early virtual machines. See
 *   [`early_vm.md`](../../../../docs/early_vm.md) for more details on early virtual machines.
 * \param service an out parameter that will be set to the service handle.
 *
 * \return
 *   - If successful, it sets `service` and returns 0.
 *   - If it fails to spawn `virtmgr`, it leaves `service` untouched and returns a negative value
 *     representing the OS error code.
 *   - If it fails to connect to the spawned `virtmgr`, it leaves `service` untouched and returns
 *     `-ECONNREFUSED`.
 */
int AVirtualizationService_create(AVirtualizationService** service, bool early);

/**
 * Destroy a VirtualizationService object.
 *
 * `AVirtualizationService_free` does nothing if `service` is null. A destroyed service object must
 * not be reused.
 *
 * \param service a handle on a virtualization service.
 */
void AVirtualizationService_free(AVirtualizationService* service);

/**
 * Represents a handle on a virtual machine.
 */
typedef struct AVirtualMachine AVirtualMachine;

/**
 * The reason why a virtual machine stopped.
 * @see AVirtualMachine_waitForStop
 */
enum StopReason : int32_t {
    /**
     * VirtualizationService died.
     */
    VIRTUALIZATION_SERVICE_DIED,
    /**
     * There was an error waiting for the virtual machine.
     */
    INFRASTRUCTURE_ERROR,
    /**
     * The virtual machine was killed.
     */
    KILLED,
    /**
     * The virtual machine stopped for an unknown reason.
     */
    UNKNOWN,
    /**
     * The virtual machine requested to shut down.
     */
    SHUTDOWN,
    /**
     * crosvm had an error starting the virtual machine.
     */
    START_FAILED,
    /**
     * The virtual machine requested to reboot, possibly as the result of a kernel panic.
     */
    REBOOT,
    /**
     * The virtual machine or crosvm crashed.
     */
    CRASH,
    /**
     * The pVM firmware failed to verify the VM because the public key doesn't match.
     */
    PVM_FIRMWARE_PUBLIC_KEY_MISMATCH,
    /**
     * The pVM firmware failed to verify the VM because the instance image changed.
     */
    PVM_FIRMWARE_INSTANCE_IMAGE_CHANGED,
    /**
     * The microdroid failed to connect to VirtualizationService's RPC server.
     */
    MICRODROID_FAILED_TO_CONNECT_TO_VIRTUALIZATION_SERVICE,
    /**
     * The payload for microdroid is changed.
     */
    MICRODROID_PAYLOAD_HAS_CHANGED,
    /**
     * The microdroid failed to verify given payload APK.
     */
    MICRODROID_PAYLOAD_VERIFICATION_FAILED,
    /**
     * The virtual machine config for microdroid is invalid (e.g. missing tasks).
     */
    MICRODROID_INVALID_PAYLOAD_CONFIG,
    /**
     * There was a runtime error while running microdroid manager.
     */
    MICRODROID_UNKNOWN_RUNTIME_ERROR,
    /**
     * The virtual machine was killed due to hangup.
     */
    HANGUP,
    /**
     * VirtualizationService sent a stop reason which was not recognised by the client library.
     */
    UNRECOGNISED,
};

/**
 * Create a virtual machine with given `config`.
 *
 * The created virtual machine is in stopped state. To run the created virtual machine, call
 * {@link AVirtualMachine_start}.
 *
 * The caller takes ownership of the returned virtual machine object, and is responsible for
 * releasing it by calling {@link AVirtualMachine_free}.
 *
 * \param service a handle on a virtualization service.
 * \param config a virtual machine config object.
 * \param consoleOutFd a writable file descriptor for the console output, or -1. Ownership will
 *   always be transferred from the caller, even if unsucessful.
 * \param consoleInFd a readable file descriptor for the console input, or -1. Ownership will always
 *   be transferred from the caller, even if unsucessful.
 * \param logFd a writable file descriptor for the log output, or -1. Ownership will always be
 *   transferred from the caller, even if unsucessful.
 * \param vm an out parameter that will be set to the virtual machine handle.
 *
 * \return If successful, it sets `vm` and returns 0. Otherwise, it leaves `vm` untouched and
 *   returns `-EIO`.
 */
int AVirtualMachine_create(const AVirtualizationService* service,
                           const AVirtualMachineConfig* config, int consoleOutFd, int consoleInFd,
                           int logFd, AVirtualMachine** vm);

/**
 * Start a virtual machine.
 *
 * \param vm a handle on a virtual machine.
 *
 * \return If successful, it returns 0. Otherwise, it returns `-EIO`.
 */
int AVirtualMachine_start(AVirtualMachine* vm);

/**
 * Stop a virtual machine.
 *
 * \param vm a handle on a virtual machine.
 *
 * \return If successful, it returns 0. Otherwise, it returns `-EIO`.
 */
int AVirtualMachine_stop(AVirtualMachine* vm);

/**
 * Wait until a virtual machine stops.
 *
 * \param vm a handle on a virtual machine.
 *
 * \return The reason why the virtual machine stopped.
 */
enum StopReason AVirtualMachine_waitForStop(AVirtualMachine* vm);

/**
 * Destroy a virtual machine.
 *
 * `AVirtualMachine_free` does nothing if `vm` is null. A destroyed virtual machine must not be
 * reused.
 *
 * \param vm a handle on a virtual machine.
 */
void AVirtualMachine_free(AVirtualMachine* vm);

__END_DECLS
