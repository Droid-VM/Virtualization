# Getting started with device assignment

Device assignment allows a VM to have direct access to HW without host/hyp
intervention.

This documents would explain how to setup and launch VM with device assignments.


## VM device assignment DTBO (a.k.a. VM DA DTBO or VM DTBO)
For device assignment, your device should provide a VM device assignment DTBO.

VM DA DTBO is a device tree overlay (a.k.a. DTBO) for device assignment. The
DTBO contains overlay nodes that describe all assignable devices. Information
includes physical reg, IOMMU, device properties and dependencies.

When a VM boots, assigned device nodes and their dependencies would be applied
to the VM's device tree. Assigned devices can read the DT and run as if they're
running on the host.

### Prepare VM DA DTBO

### VM DA DTBO Format

Devices should provide a VM DA DTBO that contains assignable devices.

Unlike normal [DTBO syntax](https://source.android.com/docs/core/architecture/dto/syntax),
VM DA DTBO expects hard coded device tree overlay syntax (i.e. do not use reference
syntax, but use `fragment@x` and `__overlay__`) because we need extra
information of the physical device nodes that wouldn't be applied to the VM DT.

Here are details and requirements:

#### Providing physical devices and physical IOMMUs

VM DA DTBO should provide a `/host` node which describes physical devices and
physical IOMMUs. The `/host` node only provides information for assigning
devices, and wouldn't be applied to VM DT. Here are details:

* Physical IOMMU nodes
  * IOMMU nodes must have a phandle to be referenced by a physical device node.
  * IOMMU nodes must have `<android,pvmfw,token>` property. The property
    describes the IOMMU token as a `<u64>`. IOMMU token is a unique identifier
    of a physical IOMMU from the hypervisor perspective. (It's meaning is also
    hypervisor-specific). IOMMU token must be constant across the guest boot for
    provisioning by pvmfw remains valid. The token must be kept up-to-date
    across hypervisor updates.
  * IOMMU nodes should have `#iommu-cells = <1>`.
    * Other `#iommu-cells` values aren't supported for now.
* Physical device nodes
  * Physical device nodes must have a `<android,pvmfw,target>` property that
    references an overlayable node. The node contains the properties that
    wouldn't be added by crosvm.
  * Physical device nodes must have `<reg>` property to provide physical
    addresses.
  * Physical device nodes can optionally contain `<iommus>` property. The
    property is prop-encoded-array and contains a number of (iommu phandle, SID)
    pairs.
    * IOMMU can be shared among devices, but should use distinct SIDs. Using the
      same SID isn't supported for now.

#### Providing assignable devices

VM DA DTBO should provide assignable devices and their labels. In DTS, labels
are defined under the `/__symbols__`. Here are details:

* Assignable devices should be backed by physical devices.
  * AVF will assign the devices with `vfio-platform`.
* Assignable devices label must point to valid 'overlayable' nodes. Otherwise
  labels are ignored.
  * Overlayable node is a node that would be applied to the base device tree
    when DTBO is applied. For normal [DTBO syntax](https://source.android.com/docs/core/architecture/dto/syntax),
    we don't recommend hard coded `__overlay__`, but it's required for VM DA
    DTBO to provide both overable nodes and non-overlayable nodes. Here are
    requirements of hard coded overlayable nedes:
    * Fragment node (`fragment@[a-z0-9]*`) should exist at the top level with
      property `target-path = "/"`.
    * We only support root as target-path to prevent unexpected modification of
      VM DT.
    * The second depth node should only have `__overlay__` node.
    * For more details about hard coded `__overlay__` syntax, see: [DT object internal](https://android.googlesource.com/platform/external/dtc/+/refs/heads/main/Documentation/dt-object-internal.txt)

#### Providing dependencies
VM DA DTBO may have dependencies via phandle reference. But it can only
reference inside of the VM DA DTBO.

When a device node is assigned, references of the node would also be applied to
VM DT.

FYI, properties of ancestor nodes would be applied, but siblings or children
node wouldn't be applied unless explicitly referenced.


#### VM DA DTBO example
Here's a simple example device tree source with four assignable devices nodes.


```dts
/dts-v1/;

/ {
    // host node describes physical devices and IOMMUs, and wouldn't be applied to VM DT
    host {
        #address-cells = <0x2>;
        #size-cells = <0x1>;
        rng {
            reg = <0x0 0x12f00000 0x1000>;
            iommus = <&iommu0 0x3>;
            android,pvmfw,target = <&rng>;
        };
        light {
            reg = <0x0 0x00f00000 0x1000>, <0x0 0x00f10000 0x1000>;
            iommus = <&iommu1 0x4>, <&iommu2 0x5>;
            android,pvmfw,target = <&light>;
        };
        led {
            reg = <0x0 0x12000000 0x1000>;
            iommus = <&iommu1 0x3>;
            android,pvmfw,target = <&led>;
        };
        bus0 {
            #address-cells = <0x1>;
            #size-cells = <0x1>;
            backlight {
                reg = <0x300 0x100>;
                android,pvmfw,target = <&backlight>;
            };
        };
        iommu0: iommu0 {
            #iommu-cells = <0x1>;
            android,pvmfw,token = <0x0 0x12e40000>;
        };
        iommu1: iommu1 {
            #iommu-cells = <0x1>;
            android,pvmfw,token = <0x0 0x40000>;
        };
        iommu2: iommu2 {
            #iommu-cells = <0x1>;
            android,pvmfw,token = <0x0 0x50000>;
        };
    };

    // Beginning of the assignable devices. Assigned devices would be applied to VM DT
    fragment@rng {
        target-path = "/";
        __overlay__ {
            rng: rng {
                compatible = "android,rng";
                android,rng,ignore-gctrl-reset;
            };
        };
    };
    fragment@sensor {
        target-path = "/";
        __overlay__ {
            light: light {
                compatible = "android,light";
                version = <0x1 0x2>;
            };
        };
    };
    fragment@led {
        target-path = "/";
        __overlay__ {
            led: led {
                compatible = "android,led";
                prop = <0x555>;
            };
        };
    };
    fragment@backlight {
        target-path = "/";
        __overlay__ {
            bus0 {
                backlight: backlight {
                    compatible = "android,backlight";
                    android,backlight,ignore-gctrl-reset;
                };
            };
        };
    };
};
```

If you compile the above with `dtc -@`, then you'll get `__symbols__` for free.
The generated `__symbols__` indicates that there are four assignable devices.

```dts
    // generated __symbols__. AVF will ignore non-overlayable nodes.
    __symbols__ {
        iommu0 = "/host/iommu0";
        iommu1 = "/host/iommu1";
        iommu2 = "/host/iommu2";
        rng = "/fragment@rng/__overlay__/rng";
        light = "/fragment@sensor/__overlay__/light";
        led = "/fragment@led/__overlay__/led";
        backlight = "/fragment@backlight/__overlay__/bus0/backlight";
    };
```

### Include VM DA DTBO
VM DA DTBO should be included in the host `dtbo.img`. It should be in its own
entry, and not in host OS's.

See [Provide VM DA DTBO index in dtbo.img](#provide-vm-da-dtbo-index-in-dtboimg)
for next step.


## Prepare AVF assignable devices XML
AVF assignable devices XML format
AVF requires assignable device information for VFIO to unbind from the host and
bind to VM. The information should be provided in an XML file.

Here's example.

```xml
<devices>
    <device>
        <kind>sensor</kind>
        <dtbo_label>light</dtbo_label>
        <sysfs_path>/sys/bus/platform/devices/16d00000.light</sysfs_path>
    </device>
</devices>
```

* `<kind>`: Device kind. Currently only used for debugging purposes and not used
  for device assignment.
* `<dtbo_label>`: Label in the VM DTBO (i.e. symbols in `__symbols__`)
* `<sysfs_path>`: Sysfs path in the host OS for VFIO to bind it to the VM. Must be
  unique in the XML.

Include AVF assignable devices XML
Include the XML file at /vendor/etc/avf/assignable_devices.xml.


## Boot with VM DA DTBO
### Provide VM DA DTBO index in dtbo.img
Bootloader should provide the VM DA DTBO index in dtbo.img with sysprop
`ro.boot.hypervisor.vm_dtbo_idx.`

AVF will read the index, read VM DA DTBO, and pass it to the crosvm when booting
a VM.

### Provide VM DA DTBO in the pvmfw config
For protected VM, bootloader must provide VM DA DTBO to the pvmfw. pvmfw will
sanitize incoming device tree with the VM DTBO.

For more detail about providing VM DA DTBO in pvmfw, see: [pvmfw/README.md](../pvmfw/README.md#configuration-data-format)


## Launch VM with device assignment
We don't support client API yet in Android-V, but you can use CLI to test device assignment.

Specify `--devices ${sysfs_path}` when booting VM. The parameter can be repeated multiple times for specifying multiple devices.

Here's an example:

```sh
adb shell /apex/com.android.virt/bin/vm run-microdroid --devices /sys/bus/platform/devices/16d00000.light
```