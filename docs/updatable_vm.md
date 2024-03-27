# Updatable VM
From Android V+, AVF (with Microdroid) supports Updatable VMs. This allows the VM instances to
remain stable even when the VM core components and payload are upgraded. This includes (but is not limited to)
update of payload apk & Microdroid OS.

## Background
The following constructs have been used (and are critical) to support Updatable VM:
1. [Secretkeeper][sk_project] is the critical piece of solution. It provides secure storage for VM's secrets. It is specified as [a HAL][secretkeeperhal] and needs to be implemented in an environment with privilege higher than protected VM.
2. [DICE Policies][dice_policy]: DICE policy is the mechanism for setting constraints on a DICE chain(i.e., identities of a VM). VM seals its secrets using DICE policies, and Secretkeeper serves as a policy verifier.
3. [AuthGraph key exchange][authgraphke]: The requests/responses between pVM and Secretkeeper are ferried via Android (which is untrusted). A cryptographically secure channel is setup using AuthGraph key exchange.

[sk_project]:
https://android.googlesource.com/platform/system/secretkeeper/
[secretkeeperhal]: https://cs.android.com/android/platform/superproject/main/+/main:hardware/interfaces/security/secretkeeper/aidl/android/hardware/security/secretkeeper/ISecretkeeper.aidl
[dice_policy]: https://android.googlesource.com/platform/system/secretkeeper/+/refs/heads/main/dice_policy/
[authgraphke]: https://cs.android.com/android/platform/superproject/main/+/main:hardware/interfaces/security/authgraph/aidl/android/hardware/security/authgraph/IAuthGraphKeyExchange.aidl


## VmSecrets::V2
Updatable VMs are achieved by changing Microdroid's secret management. It now supports `VmSecrets::V2` which is derived from 2 independently secured secrets:
1. Secretkeeper protected secret.
2. Dice Sealing CDIs (similar to legacy secrets V1).

Secretkeeper protected secret is protected against rollback of boot images i.e. VM instance rebooted with downgraded
images will not have access to these secrets. This is done using [Policy Gated Storage feature](policy_gated_storage) of Secretkeeper.
On the first boot of the VM instance, Microdroid Manager (on behalf of the VM payload) generates a random secret
& stores it in Secretkeeper & on further reboots, this is retrieved from it.
Along with this secret, a sealing policy is also stored that ensures that secrets are not released to the VM instance booted with downgraded images!

## Sealing Policy
Sealing Policy is a DICE policy on the DICE chain of the payload running in Microdroid. This is constructed by Microdroid Manager on behalf of the payload & is stored along with the secret.

A highly glossified view - Sealing policy built by Microdroid has the following constraints:
* ExactMatch on DiceCertChainInitialPayload (root public key)
* ExactMatch of INSTANCE_ID, this is present in DiceChainEntry corresponding to OS.
* For each DiceChainEntry:
    1. ExactMatch on AUTHORITY_HASH.
    2. ExactMatch on MODE (Required) - Secret should be inaccessible if any of the runtime
    configuration changes. For ex, the secrets stored with a boot stage being in Normal mode
    should be inaccessible when the same stage is booted in Debug mode.
    3. GreaterOrEqual on SECURITY_VERSION (Optional): The secrets will be accessible if version of
    any image is greater or equal to the set version. This is an optional field, certain
    components may chose to prevent booting of rollback images for ex, ABL is expected to provide
    rollback protection of pvmfw. Such components may chose to not put SECURITY_VERSION in the
    corresponding DiceChainEntry.
* For each Subcomponent on the last DiceChainEntry (which corresponds to VM payload, See
    [vm_config.cddl][vm_config_cddl]):
      - GreaterOrEqual on SECURITY_VERSION
      - ExactMatch on AUTHORITY_HASH.

The sealing policy is updated each time the secret is retrieved. This ensures the secrets are only released if the security version of the images are non-decreasing.

## Deferring rollback protection
Traditionally in Android, each boot stage is responsible for rollback protection of the next boot image. ABL has access to tamper evident storage to ensure that. VM (Android U and lower) use instance.img where the boot stages (pvmfw/Microdroid) would store information about packages they boot (exact code_hash) & on subsequent boot of the instance ensure that the same images are allowed to run. This prevented running of older images, but also prevented running newer images and hence VMs were not updatable.

Secretkeeper HAL then introduced the capability of storing secrets in a TA such that the owner of the secret ( for ex. VM) while storing it, includes a corresponding sealing policy such that only entities with Dice chain that adheres to those policies can access the secrets.

This allows the bootloaders to defer rollback protection to the payload. Host relays this intention to pVM (both pVM firmware & OS) using the property (`defer-rollback-protection`) in device tree node (`/avf/untrusted`). If this is set && the guest OS is capable of `SecretkeeperProtection` then VMs fallback to Secretkeeper based rollback protection.

### Note on legacy support
Secretkeeper is a strongly recommended but not yet mandatory in Android V. If the device does not support Secretkeeper, Microdroid will fallback to legacy secrets (`VmSecrets::V1`). These are not protected against the rollback of boot images & hence pVM firmware cannot defer rollback protection. Instance image is used to record information about the images on the first boot of the instance, and any further boot prevents any different image from running i.e, Updatable VMs are not supported.