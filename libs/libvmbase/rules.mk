LOCAL_DIR := $(GET_LOCAL_DIR)
MODULE := $(LOCAL_DIR)
MODULE_CRATE_NAME := vmbase
MODULE_SRCS := \
	$(LOCAL_DIR)/src/lib.rs \

MODULE_LIBRARY_DEPS := \
	packages/modules/Virtualization/libs/cstr \
	packages/modules/Virtualization/libs/libfdt \
	trusty/user/base/lib/liballoc-rust \
	$(call FIND_CRATE,aarch64-paging) \
	$(call FIND_CRATE,buddy_system_allocator) \
	$(call FIND_CRATE,spin) \
	$(call FIND_CRATE,log) \
	$(call FIND_CRATE,once_cell) \
	$(call FIND_CRATE,smccc) \
	$(call FIND_CRATE,static_assertions) \
	$(call FIND_CRATE,tinyvec) \
	$(call FIND_CRATE,uuid) \
	$(call FIND_CRATE,virtio-drivers) \
	$(call FIND_CRATE,zerocopy) \
	$(call FIND_CRATE,zeroize) \

include make/library.mk