/*
 * Copyright 2023 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#include "apkmanifest.hpp"

#include <android-base/logging.h>
#include <android-base/result.h>
#include <androidfw/AssetsProvider.h>
#include <androidfw/ResourceTypes.h>
#include <androidfw/StringPiece.h>
#include <androidfw/Util.h>
#include <stddef.h>
#include <stdint.h>
#include <utils/Errors.h>

#include <cstdlib>
#include <iostream>
#include <string>
#include <string_view>

using android::Asset;
using android::AssetsProvider;
using android::OK;
using android::Res_value;
using android::ResXMLParser;
using android::ResXMLTree;
using android::statusToString;
using android::StringPiece16;
using android::base::Error;
using android::base::Result;
using android::util::Utf16ToUtf8;
using std::u16string_view;
using std::unique_ptr;

struct ApkManifestInfo {
    std::string package;
    uint32_t version_code;
};

namespace {
// See https://developer.android.com/guide/topics/manifest/manifest-element
constexpr u16string_view MANIFEST_TAG_NAME{u"manifest"};
constexpr u16string_view ANDROID_NAMESPACE_URL{u"http://schemas.android.com/apk/res/android"};
constexpr u16string_view PACKAGE_ATTRIBUTE_NAME{u"package"};
constexpr u16string_view VERSION_CODE_ATTRIBUTE_NAME{u"versionCode"};

Result<void> findManifestElement(ResXMLTree& tree) {
    for (;;) {
        ResXMLParser::event_code_t event = tree.next();
        switch (event) {
            case ResXMLParser::END_DOCUMENT:
            case ResXMLParser::END_TAG:
            case ResXMLParser::TEXT:
            default:
                return Error() << "Unexpected XML parsing event: " << event;
            case ResXMLParser::BAD_DOCUMENT:
                return Error() << "Failed to parse XML: " << statusToString(tree.getError());
            case ResXMLParser::START_NAMESPACE:
            case ResXMLParser::END_NAMESPACE:
                // Not of interest, keep going.
                break;
            case ResXMLParser::START_TAG:
                // The first tag in an AndroidManifest.xml should be <manifest> (no namespace).
                // And that's actually the only tag we care about.
                if (tree.getElementNamespaceID() >= 0) {
                    return Error() << "Root element has a namespace.";
                }
                size_t nameLength = 0;
                const char16_t* nameChars = tree.getElementName(&nameLength);
                if (!nameChars) {
                    return Error() << "Missing tag name";
                }
                if (u16string_view(nameChars, nameLength) != MANIFEST_TAG_NAME) {
                    return Error() << "Unexpected tag name";
                }
                LOG(INFO) << "Found <manifest>!";
                return {};
        }
    }
}

Result<std::string> getStringAttribute(const ResXMLTree& tree, size_t index) {
    size_t len;
    const char16_t* value = tree.getAttributeStringValue(index, &len);
    if (!value) {
        return Error() << "Expected attribute to have string value";
    }
    return Utf16ToUtf8(StringPiece16(value, len));
}

Result<uint32_t> getU32Attribute(const ResXMLTree& tree, size_t index) {
    auto type = tree.getAttributeDataType(index);
    switch (type) {
        case Res_value::TYPE_INT_DEC:
        case Res_value::TYPE_INT_HEX:
            return tree.getAttributeData(index);
    }
    return Error() << "Handle this";
}

Result<unique_ptr<ApkManifestInfo>> parseManifest(const char* apk_path) {
    auto asset = AssetsProvider::CreateAssetFromFile(apk_path);
    if (!asset) return Error() << "Failed to open APK manifest";

    auto buffer = asset->getBuffer(/*aligned=*/false);
    size_t len = asset->getLength();
    LOG(INFO) << "Length is " << len;

    ResXMLTree tree;
    auto status = tree.setTo(buffer, len);
    if (status != OK) {
        return Error() << "Failed to create XML Tree: " << statusToString(status);
    }

    auto result = findManifestElement(tree);
    if (!result.ok()) return result.error();

    unique_ptr<ApkManifestInfo> info{new ApkManifestInfo{}};

    size_t count = tree.getAttributeCount();
    for (size_t i = 0; i < count; ++i) {
        size_t len;
        const char16_t* chars;

        chars = tree.getAttributeNamespace(i, &len);
        auto namespaceUrl = chars ? u16string_view(chars, len) : u16string_view();

        chars = tree.getAttributeName(i, &len);
        auto attributeName = chars ? u16string_view(chars, len) : u16string_view();

        // Check for the attributes we care about, ignore all others.
        if (namespaceUrl.empty()) {
            if (attributeName == PACKAGE_ATTRIBUTE_NAME) {
                auto result = getStringAttribute(tree, i);
                if (!result.ok()) return result.error();
                info->package = *result;
            }
        } else if (namespaceUrl == ANDROID_NAMESPACE_URL) {
            if (attributeName == VERSION_CODE_ATTRIBUTE_NAME) {
                auto result = getU32Attribute(tree, i);
                if (!result.ok()) return result.error();
                info->version_code = *result;
            }
        }
    }

    return info;
}
} // namespace

const ApkManifestInfo* extractManifestInfo(const char* apk_path) {
    // android::base::InitLogging(argv);
    LOG(INFO) << "Hello world!";

    auto result = parseManifest(apk_path);
    if (!result.ok()) {
        LOG(ERROR) << "Failed to load APK manifest from " << apk_path << ":"
                   << result.error().message();
        return nullptr;
    }
    return result->release();
}

void freeManifestInfo(const ApkManifestInfo* info) {
    delete info;
}

const char* getPackageName(const ApkManifestInfo* info) {
    return info->package.c_str();
}
