# Microdroid Signature

Microdroid Signature contains the signatures of the payloads from host.

For example, APEX packages that are passed as partitions in the payload disk image should
be listed in the Microroid Signature along with their public keys so that APEXd in the
Guest OS can verify them.

## Format

Microdroid Signature is composed of header and body.

| offset | size | description                                                    |
|--------|------|----------------------------------------------------------------|
| 0      | 4    | Header. unsigned int32: body length(L) in big endian           |
| 4      | L    | Body. A protobuf message. [schema](microdroid_signature.proto) |
