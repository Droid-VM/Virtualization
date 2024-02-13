# Getting started with device assignment

Device assignment allows a VM to have direct access to HW without host/hyp
intervention.

This document explains how to setup and launch VM with device assignments.


## VM device assignment DTBO (a.k.a. VM DTBO)
For device assignment, your device should provide a VM device assignment DTBO
(a.k.a. VM DTBO). VM DTBO is a device tree overlay (a.k.a. DTBO) which describes
all assignable devices. Information includes physical reg, IOMMU, device
properties and dependencies.

VM DTBO allows to pass extra properties of assignable platform
devices to the VM (which can't be discovered from the HW) while keeping the VMM
device-agnostic.

When the host boots, the bootloader should provide VM DTBO to both host and
pvmfw.

When a VM boots, assigned device nodes and their dependencies are applied to the
VM's device tree.

### Prepare VM DTBO

### Write VM DTS for VM DTBO

Devices should provide a VM DTBO that contains assignable devices.

DTB(O) is compiled from device tree source (DTS) with `dtc` tool. [DTBO syntax]
explains basic syntax of DTS.

[DTBO syntax]: https://source.android.com/docs/core/architecture/dto/syntax

Here are details and requirements:

#### Providing assignable devices

VM DTBO should provide assignable devices and their labels.

* VM DTBO should have assignable devices in the `&{/}`, so it can be
  overlaid onto VM DT. Assignable devices should be backed by physical device.
  * We only support overlaying onto root node (i.e. `&{/}`) to prevent
    unexpected modification of VM DT.
  * AVF will assign the devices with `vfio-platform`.
* VM DTBO should have labels for assignable devices, so AVF can recognize
  assignable device list. Labels should point to valid 'overlayable' nodes or
  ignored.
  * In DTS, labels are defined under the `/__symbols__`, but `dtc -@` would auto
    generate the node for free.
  * Overlayable node is a node that would be applied to the base device tree
    when DTBO is applied.

#### Providing physical devices and physical IOMMUs

VM DTBO should provide a `/host` node which describes physical devices and
physical IOMMUs. The `/host` node only provides information for assigning
devices, and wouldn't be applied to VM DT. Here are details:

* Physical IOMMU nodes
  * IOMMU nodes must have a phandle to be referenced by a physical device node.
  * IOMMU nodes must have `<android,pvmfw,token>` property. The property
    describes the IOMMU token as a `<u64>`. IOMMU token is a unique identifier
    of a physical IOMMU from the hypervisor perspective. (It's meaning is also
    hypervisor-specific). IOMMU token must be constant across the VM boot for
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

#### Providing dependencies
VM DTBO may have dependencies via phandle reference. But it can only reference
inside of the VM DTBO.

When a device node is assigned, dependencies of the node would also be applied
to VM DT.

FYI, properties of ancestor nodes would be applied, but siblings or children
node wouldn't be applied unless explicitly referenced.


#### VM DTBO example
Here's a simple example device tree source with four assignable devices nodes.


```dts
/dts-v1/;
/plugin/;

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
};

// Beginning of the assignable devices. Assigned devices would be applied to VM DT

&{/} {  // We only allows to overlay to root node
    rng: rng {
        compatible = "android,rng";
        android,rng,ignore-gctrl-reset;
    };
    light: light {
        compatible = "android,light";
        version = <0x1 0x2>;
    };
    led: led {
        compatible = "android,led";
        prop = <0x555>;
    };
    bus0 {
        backlight: backlight {
            compatible = "android,backlight";
            android,backlight,ignore-gctrl-reset;
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

### Include VM DTBO in image
VM DTBO should be included in the host `dtbo.img`. It should be in its own
entry, and not togher with any host OS's.

See [Provide VM DTBO index in dtbo.img](#provide-vm-dtbo-index-in-dtboimg)
for next step.


## Prepare AVF assignable devices XML
### AVF assignable devices XML format
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

### Include AVF assignable devices XML
Include the XML file at `/vendor/etc/avf/assignable_devices.xml`.


## Boot with VM DTBO
Bootloader should provide VM DTBO to both host and pvmfw.

### Provide VM DTBO index in dtbo.img
Bootloader should provide the VM DTBO index with sysprop
`ro.boot.hypervisor.vm_dtbo_idx.`. DTBO index represents DTBO location in
dtbo.img, and also used to provide DTBO of host OS. See [DTB/DTBO Paritions]
for partition format.

AVF reads the index, read VM DTBO, and pass it to the crosvm when booting a VM.

[DTB/DTBO Paritions]: https://source.android.com/docs/core/architecture/dto/partitions

### Provide VM DTBO in the pvmfw config
For protected VM, bootloader must provide VM DTBO to the pvmfw. pvmfw will
sanitize incoming device tree with the VM DTBO.

For more detail about providing VM DTBO in pvmfw,
see: [pvmfw/README.md](../pvmfw/README.md#configuration-data-format)


## Launch VM with device assignment
We don't support client API yet in Android-V, but you can use CLI to test device
assignment.

Specify `--devices ${sysfs_path}` when booting VM. The parameter can be repeated
multiple times for specifying multiple devices.

Here's an example:

```sh
adb shell /apex/com.android.virt/bin/vm run-microdroid --devices /sys/bus/platform/devices/16d00000.light
```