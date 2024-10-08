// Copyright 2024 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Copied from ChromiumOS with relicensing:
// src/platform2/libchromeos-rs/poll_token_derive/src/tests.rs

use quote::quote;
use syn::{parse_quote, DeriveInput};

#[test]
fn test_variant_bits() {
    let mut variants = vec![parse_quote!(A)];
    assert_eq!(crate::variant_bits(&variants), 0);

    variants.push(parse_quote!(B));
    variants.push(parse_quote!(C));
    assert_eq!(crate::variant_bits(&variants), 2);

    for _ in 0..1021 {
        variants.push(parse_quote!(Dynamic));
    }
    assert_eq!(crate::variant_bits(&variants), 10);

    variants.push(parse_quote!(OneMore));
    assert_eq!(crate::variant_bits(&variants), 11);
}

#[test]
fn poll_token_e2e() {
    let input: DeriveInput = parse_quote! {
        enum Token {
            A,
            B,
            C,
            D(usize),
            E { foobaz: u32 },
        }
    };

    let actual = crate::poll_token_inner(input);
    let expected = quote! {
        impl PollToken for Token {
            fn as_raw_token(&self) -> u64 {
                match *self {
                    Token::A => 0u64,
                    Token::B => 1u64,
                    Token::C => 2u64,
                    Token::D { 0: data } => 3u64 | ((data as u64) << 3u32),
                    Token::E { foobaz: data } => 4u64 | ((data as u64) << 3u32),
                }
            }

            fn from_raw_token(data: u64) -> Self {
                match data & 7u64 {
                    0u64 => Token::A,
                    1u64 => Token::B,
                    2u64 => Token::C,
                    3u64 => Token::D { 0: (data >> 3u32) as usize },
                    4u64 => Token::E { foobaz: (data >> 3u32) as u32 },
                    _ => unreachable!(),
                }
            }
        }
    };

    assert_eq!(actual.to_string(), expected.to_string());
}
