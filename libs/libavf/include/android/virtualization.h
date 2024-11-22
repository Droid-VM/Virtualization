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

#ifdef __cplusplus
extern "C" {
#endif

typedef struct AVirtualMachineConfig AVirtualMachineConfig;

enum CpuTopology : int32_t {
    ONE_CPU,
    MATCH_HOST,
};

AVirtualMachineConfig* AVirtualMachineConfig_createRaw();
void AVirtualMachineConfig_free(AVirtualMachineConfig* config);
int AVirtualMachineConfig_setName(AVirtualMachineConfig* config, const char* name);
int AVirtualMachineConfig_setInstanceId(AVirtualMachineConfig* config,
                                        const int8_t* instanceId); // instanceId size must be 64
int AVirtualMachineConfig_setKernel(AVirtualMachineConfig* config, int fd);
int AVirtualMachineConfig_setInitRd(AVirtualMachineConfig* config, int fd);
int AVirtualMachineConfig_addDisk(AVirtualMachineConfig* config, int fd);
int AVirtualMachineConfig_setMemoryMib(AVirtualMachineConfig* config, int32_t memory_mib);
int AVirtualMachineConfig_setDeviceTreeOverlay(AVirtualMachineConfig* config, const char* path);
int AVirtualMachineConfig_setProtectedVm(AVirtualMachineConfig* config, bool protectedVm);
int AVirtualMachineConfig_setBalloon(AVirtualMachineConfig* config, bool balloon);
int AVirtualMachineConfig_setCpuTopology(AVirtualMachineConfig* config,
                                         enum CpuTopology cpuTopology);
int AVirtualMachineConfig_setHypervisorSpecificAuthMethod(AVirtualMachineConfig* config,
                                                          bool enable);
int AVirtualMachineConfig_addCustomMemoryBackingFile(AVirtualMachineConfig* config, int fd,
                                                     size_t rangeStart, size_t rangeEnd);
int AVirtualMachineConfig_addReservedMmioRange(AVirtualMachineConfig* config, size_t rangeStart,
                                               size_t rangeEnd);

typedef struct AVirtualizationService AVirtualizationService;

int AVirtualizationService_create(AVirtualizationService** service);
int AVirtualizationService_createEarly(AVirtualizationService** service);
void AVirtualizationService_free(AVirtualizationService* service);

typedef struct AVirtualMachine AVirtualMachine;

enum StopReason : int32_t {
    // VirtualizationService died.
    VIRTUALIZATION_SERVICE_DIED,
    // There was an error waiting for the VM.
    INFRASTRUCTURE_ERROR,
    // The VM was killed.
    KILLED,
    // The VM died for an unknown reason.
    UNKNOWN,
    // The VM requested to shut down.
    SHUTDOWN,
    // crosvm had an error starting the VM.
    START_FAILED,
    // The VM requested to reboot, possibly as the result of a kernel panic.
    REBOOT,
    // The VM or crosvm crashed.
    CRASH,
    // The pVM firmware failed to verify the VM because the public key doesn't match.
    PVM_FIRMWARE_PUBLIC_KEY_MISMATCH,
    // The pVM firmware failed to verify the VM because the instance image changed.
    PVM_FIRMWARE_INSTANCE_IMAGE_CHANGED,
    // The microdroid failed to connect to VirtualizationService's RPC server.
    MICRODROID_FAILED_TO_CONNECT_TO_VIRTUALIZATION_SERVICE,
    // The payload for microdroid is changed.
    MICRODROID_PAYLOAD_HAS_CHANGED,
    // The microdroid failed to verify given payload APK.
    MICRODROID_PAYLOAD_VERIFICATION_FAILED,
    // The VM config for microdroid is invalid (e.g. missing tasks).
    MICRODROID_INVALID_PAYLOAD_CONFIG,
    // There was a runtime error while running microdroid manager.
    MICRODROID_UNKNOWN_RUNTIME_ERROR,
    // The VM was killed due to hangup.
    HANGUP,
    // VirtualizationService sent a death reason which was not recognised by the client library.
    UNRECOGNISED,
};

int AVirtualMachine_create(const AVirtualizationService* service,
                           const AVirtualMachineConfig* config, int consoleOutFd, int consoleInFd,
                           int logFd, AVirtualMachine** vm);
int AVirtualMachine_start(AVirtualMachine* vm);
int AVirtualMachine_stop(AVirtualMachine* vm);
enum StopReason AVirtualMachine_waitForStop(AVirtualMachine* vm);
void AVirtualMachine_free(AVirtualMachine* vm);

#ifdef __cplusplus
}
#endif
