#include <android-base/logging.h>
#include <android-base/result.h>
#include <androidfw/AssetsProvider.h>
#include <androidfw/ResourceTypes.h>
#include <androidfw/StringPiece.h>
#include <androidfw/Util.h>
#include <utils/Errors.h>

#include <cstdlib>
#include <iostream>
#include <string>

using android::AssetsProvider;
using android::OK;
using android::Res_value;
using android::ResXMLParser;
using android::ResXMLTree;
using android::statusToString;
using android::StringPiece16;
using android::base::Error;
using android::base::Result;

std::string to_utf8(const char16_t* str16, size_t len) {
    return str16 ? android::util::Utf16ToUtf8(StringPiece16(str16, len)) : "";
}

std::string get_utf8(ResXMLTree const& tree,
                     const char16_t* (ResXMLTree::*get)(size_t* outLen) const) {
    size_t len;
    auto str16 = (tree.*get)(&len);
    return to_utf8(str16, len);
}

std::string get_utf8(ResXMLTree const& tree, size_t idx,
                     const char16_t* (ResXMLTree::*get)(size_t idx, size_t* outLen) const) {
    size_t len;
    auto str16 = (tree.*get)(idx, &len);
    return to_utf8(str16, len);
}

Result<void> log_version(std::string const& path) {
    auto asset = AssetsProvider::CreateAssetFromFile(path);
    if (!asset) {
        return Error() << "Failed to create asset from " << path;
    }

    auto buffer = asset->getBuffer(/*aligned=*/false); // Does it matter?
    size_t len = asset->getLength();
    LOG(INFO) << "Length is " << len;

    ResXMLTree tree;
    auto status = tree.setTo(buffer, len);
    if (status != OK) {
        return Error() << "Failed to create XML Tree: " << statusToString(status);
    }

    for (;;) {
        ResXMLParser::event_code_t event = tree.next();
        if (event == ResXMLParser::END_DOCUMENT) {
            break;
        };
        switch (event) {
            case ResXMLParser::BAD_DOCUMENT: {
                return Error() << "Failed to parse XML: " << statusToString(tree.getError());
            }
            case ResXMLParser::START_NAMESPACE: {
                LOG(INFO) << "START_NAMESPACE " << tree.getNamespacePrefixID();
                break;
            }
            case ResXMLParser::END_NAMESPACE: {
                LOG(INFO) << "END_NAMESPACE " << tree.getNamespacePrefixID();
                break;
            }
            case ResXMLParser::START_TAG: {
                LOG(INFO) << "START_TAG "
                          << " namespaceID " << tree.getElementNamespaceID() << " nameID "
                          << tree.getElementNameID() << " namespace "
                          << get_utf8(tree, &ResXMLTree::getElementNamespace) << " name "
                          << get_utf8(tree, &ResXMLTree::getElementName);
                size_t count = tree.getAttributeCount();
                for (size_t i = 0; i < count; ++i) {
                    auto type = tree.getAttributeDataType(i);
                    LOG(INFO) << "  Attribute namespaceID " << tree.getAttributeNamespaceID(i)
                              << " namespace "
                              << get_utf8(tree, i, &ResXMLTree::getAttributeNamespace) << " nameID "
                              << tree.getAttributeNameID(i) << " name "
                              << get_utf8(tree, i, &ResXMLTree::getAttributeName) << " type "
                              << type;
                    switch (type) {
                        case Res_value::TYPE_STRING: {
                            LOG(INFO) << "    String: "
                                      << get_utf8(tree, i, &ResXMLTree::getAttributeStringValue);
                            break;
                        }
                        case Res_value::TYPE_INT_DEC:
                        case Res_value::TYPE_INT_HEX: {
                            LOG(INFO) << "    Number: " << tree.getAttributeData(i);
                            break;
                        }
                    }
                }
                break;
            }
            case ResXMLParser::END_TAG: {
                LOG(INFO) << "END_TAG "
                          << " namespaceID " << tree.getElementNamespaceID() << " nameID "
                          << tree.getElementNameID() << " namespace "
                          << get_utf8(tree, &ResXMLTree::getElementNamespace) << " name "
                          << get_utf8(tree, &ResXMLTree::getElementName);
                break;
            }
            case ResXMLParser::TEXT: {
                LOG(INFO) << "TEXT ID " << tree.getTextID() << " text "
                          << get_utf8(tree, &ResXMLTree::getText);
                break;
            }
            default: {
                LOG(INFO) << "Got unexpected event: " << event;
                break;
            }
        }
    }
    return {};
}

int main(int argc, char* argv[]) {
    if (argc != 2) {
        std::cout << "Usage:\n";
        std::cout << "    " << argv[0] << " <Manifest File>\n";
        return EXIT_FAILURE;
    }

    android::base::InitLogging(argv);
    LOG(INFO) << "Hello world!";

    auto result = log_version(argv[1]);
    if (!result.ok()) {
        LOG(ERROR) << result.error().message();
        return EXIT_FAILURE;
    }

    return 0;
}
