LOCAL_DIR := $(GET_LOCAL_DIR)
MODULE := $(LOCAL_DIR)
MODULE_CRATE_NAME := cstr
MODULE_SRCS := \
	$(LOCAL_DIR)/src/lib.rs \

include make/library.mk