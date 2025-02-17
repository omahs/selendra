// Copyright (C) 2021-2022 Selendra.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use crate::{SubSysAttrItem, SubSystemAttrItems};
use assert_matches::assert_matches;
use quote::quote;
use syn::parse_quote;

mod attr {
	use super::*;

	#[test]
	fn attr_full_works() {
		let attr: OrchestraAttrArgs = parse_quote! {
			gen=AllMessage, event=::some::why::ExternEvent, signal=SigSigSig, signal_capacity=111, message_capacity=222,
			error=OrchestraError,
		};
		assert_matches!(attr, OrchestraAttrArgs {
			message_channel_capacity,
			signal_channel_capacity,
			..
		} => {
			assert_eq!(message_channel_capacity, 222);
			assert_eq!(signal_channel_capacity, 111);
		});
	}

	#[test]
	fn attr_partial_works() {
		let attr: OrchestraAttrArgs = parse_quote! {
			gen=AllMessage, event=::some::why::ExternEvent, signal=::foo::SigSigSig,
			error=OrchestraError,
		};
		assert_matches!(attr, OrchestraAttrArgs {
			message_channel_capacity: _,
			signal_channel_capacity: _,
			..
		} => {
		});
	}
}

mod strukt {

	use super::*;

	#[test]
	fn parse_subsystem_attr_item_works_00_wip() {
		assert_matches!(
		syn::parse2::<SubSysAttrItem>(quote! {
			wip
		}), Ok(SubSysAttrItem::Wip(_)) => {
		});
	}

	#[test]
	fn parse_subsystem_attr_item_works_02_sends() {
		assert_matches!(
		syn::parse2::<SubSysAttrItem>(quote! {
			sends: [A, B, C]
		}), Ok(SubSysAttrItem::Sends(sends)) => {
			assert_eq!(sends.sends.len(), 3);
		});
	}

	#[test]
	fn parse_subsystem_attr_item_works_03_sends() {
		assert_matches!(
		syn::parse2::<SubSysAttrItem>(quote! {
			sends: [A]
		}), Ok(SubSysAttrItem::Sends(sends)) => {
			assert_eq!(sends.sends.len(), 1);
		});
	}

	#[test]
	fn parse_subsystem_attr_item_works_04_sends() {
		assert_matches!(
		syn::parse2::<SubSysAttrItem>(quote! {
			sends: [A,]
		}), Ok(SubSysAttrItem::Sends(sends)) => {
			assert_eq!(sends.sends.len(), 1);
		});
	}

	#[test]
	fn parse_subsystem_attr_item_works_05_sends() {
		assert_matches!(
		syn::parse2::<SubSysAttrItem>(quote! {
			sends: []
		}), Ok(SubSysAttrItem::Sends(sends)) => {
			assert_eq!(sends.sends.len(), 0);
		});
	}

	#[test]
	fn parse_subsystem_attr_item_works_06_consumes() {
		assert_matches!(
		syn::parse2::<SubSysAttrItem>(quote! {
			consumes: Foo
		}), Ok(SubSysAttrItem::Consumes(_consumes)) => {
		});
	}

	#[test]
	fn parse_subsystem_attr_item_works_07_consumes() {
		assert_matches!(
		syn::parse2::<SubSysAttrItem>(quote! {
			Foo
		}), Ok(SubSysAttrItem::Consumes(_consumes)) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_00() {
		syn::parse2::<SubSystemAttrItems>(quote! {
			(wip, blocking, consumes: Foo, sends: [])
		})
		.unwrap();
	}

	#[test]
	fn parse_subsystem_attributes_works_01() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(blocking, Foo, sends: [])
		}), Ok(_) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_02() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(consumes: Foo, sends: [Bar])
		}), Ok(_) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_03() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(blocking, consumes: Foo, sends: [Bar])
		}), Ok(_) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_04() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(wip, consumes: Foo, sends: [Bar])
		}), Ok(_) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_05() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(consumes: Foo)
		}), Ok(_) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_06() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(sends: [Foo], consumes: Bar)
		}), Ok(_) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_07_duplicate_send() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(sends: [Foo], Bar, Y)
		}), Err(e) => {
			dbg!(e)
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_08() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(sends: [Foo], consumes: Bar)
		}), Ok(_) => {
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_09_neither_consumes_nor_sends() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(sends: [])
		}), Err(e) => {
			// must either consume smth or sends smth, neither is NOK
			dbg!(e)
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_10_empty_with_braces() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			()
		}), Err(e) => {
			dbg!(e)
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_11_empty() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {

		}), Err(e) => {
			dbg!(e)
		});
	}

	#[test]
	fn parse_subsystem_attributes_works_12_duplicate_consumes_different_fmt() {
		assert_matches!(
		syn::parse2::<SubSystemAttrItems>(quote! {
			(Foo, consumes = Foo)
		}), Err(e) => {
			dbg!(e)
		});
	}

	#[test]
	fn struct_parse_baggage() {
		let item: OrchestraGuts = parse_quote! {
			pub struct Ooooh<X = Pffffffft> where X: Secrit {
				#[subsystem(consumes: Foo, sends: [])]
				sub0: FooSubsystem,

				metrics: Metrics,
			}
		};
		let _ = dbg!(item);
	}

	#[test]
	fn struct_parse_full() {
		let item: OrchestraGuts = parse_quote! {
			pub struct Ooooh<X = Pffffffft> where X: Secrit {
				#[subsystem(consumes: Foo, sends: [])]
				sub0: FooSubsystem,

				#[subsystem(blocking, consumes: Bar, sends: [])]
				yyy: BaersBuyBilliardBalls,

				#[subsystem(blocking, consumes: Twain, sends: [])]
				fff: Beeeeep,

				#[subsystem(consumes: Rope)]
				mc: MountainCave,

				metrics: Metrics,
			}
		};
		let _ = dbg!(item);
	}

	#[test]
	fn struct_parse_basic() {
		let item: OrchestraGuts = parse_quote! {
			pub struct Ooooh {
				#[subsystem(consumes: Foo, sends: [])]
				sub0: FooSubsystem,
			}
		};
		let _ = dbg!(item);
	}
}
