# Service VM

The Service VM is a lightweight, bare-metal virtual machine specifically
designed to run various services for other virtual machines. It fulfills the
following requirements:

-   The secret within the instance image remains constant across updates of the
    client VMs and the Service VM.
-   Only one instance of the Service VM is allowed to run at any given time.

## RKP VM (Remote Key Provisioning Virtual Machine)

The RKP VM is a key dependency of the Service VM. It is a virtual machine that
undergoes validation by the [RKP][rkp] Server and acts as a remotely provisioned
component for verifying the integrity of other virtual machines.

[rkp]: https://source.android.com/docs/core/ota/modular-system/remote-key-provisioning

### RKP VM attestation

The RKP VM is recognized and attested by the RKP server, which acts as a trusted
entity responsible for verifying the DICE chain of the RKP VM. This verification
ensures that the RKP VM is operating on a genuine device.
Additionally, the RKP VM is validated by the [pVM Firmware][pvmfw], as part of
the verified boot process.

[pvmfw]: https://android.googlesource.com/platform/packages/modules/Virtualization/+/main/pvmfw/README.md

### Client VM attestation

Once the RKP VM is successfully attested, it assumes the role of a trusted
platform to attest client VMs. It leverages its trusted status to validate the
integrity of the [DICE chain][open-dice] associated with each client VM. This
validation process verifies that the client VMs are running in the expected
[Microdroid][microdroid] VM environment, and certifies the payload executed
within the VM. Currently, only Microdroid VMs are supported.

[open-dice]: https://android.googlesource.com/platform/external/open-dice/+/main/docs/android.md
[microdroid]: https://android.googlesource.com/platform/packages/modules/Virtualization/+/main/microdroid/README.md
