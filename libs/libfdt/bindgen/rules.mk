LOCAL_DIR := $(GET_LOCAL_DIR)
MODULE := $(LOCAL_DIR)
MODULE_CRATE_NAME := libfdt_bindgen
MODULE_SRCS := \
	$(LOCAL_DIR)/lib.rs \

MODULE_LIBRARY_DEPS := \
	$(call FIND_CRATE,static_assertions) \

# TODO: find out why we can't pass --use-core here, does it get appended elsewhere?
MODULE_BINDGEN_FLAGS := \
	--allowlist-type=fdt_.* \
    --allowlist-function=fdt_.* \
    --allowlist-var=FDT_.* \
    --raw-line=#![no_std] \
    --ctypes-prefix=core::ffi \

MODULE_BINDGEN_SRC_HEADER := $(LOCAL_DIR)/fdt.h

include make/library.mk