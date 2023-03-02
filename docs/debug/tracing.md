# Hypervisor & guest tracing

## Hypervisor tracing

Starting with android14-5.15 kernel it is possible to get traces from the hypervisor.
The hypervisor tracing infrastructure is very similar to the regular kernel tracing one (ftrace),
however there are some differences.

TODO(b/249050813): Stay tuned, more docs coming soon!

## Microdroid VM tracing

Unfortunately, there are some limitations on Microdroid VM tracing:

* It is not possible to simultaneously capture from the host Android & guest Microdroid VM.
* Only atrace is supported for capturing traces from the Microdroid VM.

### Capturing traces from running VM

TODO(ioffe): fill in

### Capturing traces during VM boot

TODO(ioffe): fill in