LOCAL_DIR := $(GET_LOCAL_DIR)
MODULE := $(LOCAL_DIR)
MODULE_CRATE_NAME := libfdt
MODULE_SRCS := \
	$(LOCAL_DIR)/src/lib.rs \

MODULE_LIBRARY_DEPS := \
	packages/modules/Virtualization/libs/cstr \
	$(LOCAL_DIR)/bindgen \
	$(call FIND_CRATE,static_assertions) \
	$(call FIND_CRATE,zerocopy) \

include make/library.mk