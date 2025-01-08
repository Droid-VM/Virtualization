LOCAL_DIR := $(GET_LOCAL_DIR)

MODULE := $(LOCAL_DIR)

MODULE_SRCS := $(LOCAL_DIR)/src/lib.rs

MODULE_CRATE_NAME := dice_sample_inputs

MODULE_LIBRARY_DEPS += \
	$(call FIND_CRATE,coset) \
	$(call FIND_CRATE,ciborium) \
	$(call FIND_CRATE,log) \
	packages/modules/Virtualization/libs/dice/open_dice \

include make/library.mk
