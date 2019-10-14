// Copyright 2019 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

//! Generates the extern host function declarations as well as the implementation for these host
//! functions. The implementation of these host functions will call the native bare functions.

use crate::utils::{
	generate_crate_access, create_host_function_ident, get_function_argument_names,
	get_function_argument_types_without_ref, get_function_argument_types_ref_and_mut,
	get_function_argument_names_and_types_without_ref, get_trait_methods,
};

use syn::{
	ItemTrait, TraitItemMethod, Result, ReturnType, Ident, TraitItem, Pat, Error, Signature,
	spanned::Spanned,
};

use proc_macro2::{TokenStream, Span};

use quote::{quote, ToTokens};

use inflector::Inflector;

use std::iter::Iterator;

/// Generate the extern host functions for wasm and the `HostFunctions` struct that provides the
/// implementations for the host functions on the host.
pub fn generate(trait_def: &ItemTrait) -> Result<TokenStream> {
	let trait_name = &trait_def.ident;
	let extern_host_function_impls = get_trait_methods(trait_def)
		.try_fold(TokenStream::new(), |mut t, m| {
			t.extend(generate_extern_host_function(m, trait_name)?);
			Ok::<_, Error>(t)
		})?;
	let extern_host_exchangeable_functions = get_trait_methods(trait_def)
		.try_fold(TokenStream::new(), |mut t, m| {
			t.extend(generate_extern_host_exchangeable_function(m, trait_name)?);
			Ok::<_, Error>(t)
		})?;
	let host_functions_struct = generate_host_functions_struct(trait_def)?;

	Ok(
		quote! {
			/// The implementations of the extern host functions. This special implementation module
			/// is required to change the extern host functions signature to
			/// `unsafe fn name(args) -> ret` to make the function implementations exchangeable.
			#[cfg(not(feature = "std"))]
			pub mod extern_host_function_impls {
				use super::*;

				#extern_host_function_impls
			}

			#extern_host_exchangeable_functions

			#host_functions_struct
		}
	)
}

/// Generate the extern host function for the given method.
fn generate_extern_host_function(method: &TraitItemMethod, trait_name: &Ident) -> Result<TokenStream> {
	let crate_ = generate_crate_access();
	let arg_types = get_function_argument_types_without_ref(&method.sig);
	let arg_types2 = get_function_argument_types_without_ref(&method.sig);
	let arg_names = get_function_argument_names(&method.sig);
	let arg_names2 = get_function_argument_names(&method.sig);
	let arg_names3 = get_function_argument_names(&method.sig);
	let function = create_host_function_ident(&method.sig.ident, trait_name);
	let doc_string = format!(" Default extern host function implementation for [`../{}`].", function);

	let output = match method.sig.output {
		ReturnType::Default => quote!(),
		ReturnType::Type(_, ref ty) => quote! {
			-> <#ty as #crate_::RIType>::FFIType
		}
	};

	Ok(
		quote! {
			#[doc(#doc_string)]
			pub unsafe fn #function (
				#( #arg_names: <#arg_types as #crate_::RIType>::FFIType ),*
			) #output {
				mod implementation {
					use super::*;

					extern "C" {
						pub fn #function (
							#( #arg_names2: <#arg_types2 as #crate_::RIType>::FFIType ),*
						) #output;
					}
				}

				implementation::#function( #( #arg_names3 ),* )
			}
		}
	)
}

/// Generate the extern host exchangeable function for the given method.
fn generate_extern_host_exchangeable_function(
	method: &TraitItemMethod,
	trait_name: &Ident,
) -> Result<TokenStream> {
	let crate_ = generate_crate_access();
	let arg_types = get_function_argument_types_without_ref(&method.sig);
	let function = create_host_function_ident(&method.sig.ident, trait_name);
	let doc_string = format!(" Exchangeable extern host function used by [`{}`].", method.sig.ident);

	let output = match method.sig.output {
		ReturnType::Default => quote!(),
		ReturnType::Type(_, ref ty) => quote! {
			-> <#ty as #crate_::RIType>::FFIType
		}
	};

	Ok(
		quote! {
			#[cfg(not(feature = "std"))]
			#[allow(non_upper_case_globals)]
			#[doc(#doc_string)]
			pub static #function : #crate_::wasm::ExchangeableFunction<
				unsafe fn ( #( <#arg_types as #crate_::RIType>::FFIType ),* ) #output
			> = #crate_::wasm::ExchangeableFunction::new(extern_host_function_impls::#function);
		}
	)
}

/// Generate the `HostFunctions` struct that implements `wasm-interface::HostFunctions` to provide
/// implementations for the extern host functions.
fn generate_host_functions_struct(trait_def: &ItemTrait) -> Result<TokenStream> {
	let crate_ = generate_crate_access();
	let host_functions = trait_def
		.items
		.iter()
		.filter_map(|i| match i {
			TraitItem::Method(ref method) => Some(method),
			_ => None,
		})
		.map(|m| generate_host_function_implementation(&trait_def.ident, m))
		.collect::<Result<Vec<_>>>()?;
	let host_functions_count = trait_def
		.items
		.iter()
		.filter(|i| match i {
			TraitItem::Method(_) => true,
			_ => false,
		})
		.count();

	Ok(
		quote! {
			/// Provides implementations for the extern host functions.
			#[cfg(feature = "std")]
			pub struct HostFunctions;

			#[cfg(feature = "std")]
			impl #crate_::wasm_interface::HostFunctions for HostFunctions {
				fn get_function(index: usize) -> Option<&'static dyn #crate_::wasm_interface::Function> {
					[ #( #host_functions ),* ].get(index).map(|f| *f)
				}

				fn num_functions() -> usize {
					#host_functions_count
				}
			}
		}
	)
}

/// Generates the host function struct that implements `wasm_interface::Function` and returns a static
/// reference to this struct.
///
/// When calling from wasm into the host, we will call the `execute` function that calls the native
/// implementation of the function.
fn generate_host_function_implementation(
	trait_name: &Ident,
	method: &TraitItemMethod,
) -> Result<TokenStream> {
	let name = create_host_function_ident(&method.sig.ident, trait_name).to_string();
	let struct_name = Ident::new(&name.to_pascal_case(), Span::call_site());
	let crate_ = generate_crate_access();
	let signature = generate_wasm_interface_signature_for_host_function(&method.sig)?;
	let wasm_to_ffi_values = generate_wasm_to_ffi_values(&method.sig).collect::<Result<Vec<_>>>()?;
	let ffi_to_host_values = generate_ffi_to_host_value(&method.sig).collect::<Result<Vec<_>>>()?;
	let host_function_call = generate_host_function_call(&method.sig);
	let into_preallocated_ffi_value = generate_into_preallocated_ffi_value(&method.sig)?;
	let convert_return_value = generate_return_value_into_wasm_value(&method.sig);

	Ok(
		quote! {
			{
				struct #struct_name;

				#[allow(unused)]
				impl #crate_::wasm_interface::Function for #struct_name {
					fn name(&self) -> &str {
						#name
					}

					fn signature(&self) -> #crate_::wasm_interface::Signature {
						#signature
					}

					fn execute(
						&self,
						context: &mut dyn #crate_::wasm_interface::FunctionContext,
						args: &mut dyn Iterator<Item = #crate_::wasm_interface::Value>,
					) -> std::result::Result<Option<#crate_::wasm_interface::Value>, String> {
						#( #wasm_to_ffi_values )*
						#( #ffi_to_host_values )*
						#host_function_call
						#into_preallocated_ffi_value
						#convert_return_value
					}
				}

				&#struct_name as &dyn #crate_::wasm_interface::Function
			}
		}
	)
}

/// Generate the `wasm_interface::Signature` for the given host function `sig`.
fn generate_wasm_interface_signature_for_host_function(sig: &Signature) -> Result<TokenStream> {
	let crate_ = generate_crate_access();
	let return_value = match &sig.output {
		ReturnType::Type(_, ty) =>
			quote! {
				Some( <<#ty as #crate_::RIType>::FFIType as #crate_::wasm_interface::IntoValue>::VALUE_TYPE )
			},
		ReturnType::Default => quote!( None ),
	};
	let arg_types = get_function_argument_types_without_ref(sig)
		.map(|ty| quote! {
			<<#ty as #crate_::RIType>::FFIType as #crate_::wasm_interface::IntoValue>::VALUE_TYPE
		});

	Ok(
		quote! {
			#crate_::wasm_interface::Signature {
				args: std::borrow::Cow::Borrowed(&[ #( #arg_types ),* ][..]),
				return_value: #return_value,
			}
		}
	)
}

/// Generate the code that converts the wasm values given to `HostFunctions::execute` into the FFI
/// values.
fn generate_wasm_to_ffi_values<'a>(
	sig: &'a Signature,
) -> impl Iterator<Item = Result<TokenStream>> + 'a {
	let crate_ = generate_crate_access();
	let function_name = &sig.ident;
	let error_message = format!(
		"Number of arguments given to `{}` does not match the expected number of arguments!",
		function_name,
	);

	get_function_argument_names_and_types_without_ref(sig)
		.map(move |(name, ty)| {
			let try_from_error = format!(
				"Could not instantiate `{}` from wasm value while executing `{}`!",
				name.to_token_stream(),
				function_name,
			);

			let var_name = generate_ffi_value_var_name(name)?;

			Ok(quote! {
				let val = args.next().ok_or_else(|| #error_message)?;
				let #var_name = <
					<#ty as #crate_::RIType>::FFIType as #crate_::wasm_interface::TryFromValue
				>::try_from_value(val).ok_or_else(|| #try_from_error)?;
			})
		})
}

/// Generate the code to convert the ffi values on the host to the host values using `FromFFIValue`.
fn generate_ffi_to_host_value<'a>(
	sig: &'a Signature,
) -> impl Iterator<Item = Result<TokenStream>> + 'a {
	let mut_access = get_function_argument_types_ref_and_mut(sig);
	let crate_ = generate_crate_access();

	get_function_argument_names_and_types_without_ref(sig)
		.zip(mut_access.map(|v| v.and_then(|m| m.1)))
		.map(move |((name, ty), mut_access)| {
			let ffi_value_var_name = generate_ffi_value_var_name(name)?;

			Ok(
				quote! {
					let #mut_access #name = <#ty as #crate_::host::FromFFIValue>::from_ffi_value(
						context,
						#ffi_value_var_name,
					)?;
				}
			)
		})
}

/// Generate the code to call the host function and the ident that stores the result.
fn generate_host_function_call(sig: &Signature) -> TokenStream {
	let host_function_name = &sig.ident;
	let result_var_name = generate_host_function_result_var_name(&sig.ident);
	let ref_and_mut = get_function_argument_types_ref_and_mut(sig).map(|ram|
		ram.map(|(vr, vm)| quote!(#vr #vm))
	);
	let names = get_function_argument_names(sig);

	let var_access = names.zip(ref_and_mut).map(|(n, ref_and_mut)| {
		quote!( #ref_and_mut #n )
	});

	quote! {
		let #result_var_name = #host_function_name ( #( #var_access ),* );
	}
}

/// Generate the variable name that stores the result of the host function.
fn generate_host_function_result_var_name(name: &Ident) -> Ident {
	Ident::new(&format!("{}_result", name), Span::call_site())
}

/// Generate the variable name that stores the FFI value.
fn generate_ffi_value_var_name(pat: &Pat) -> Result<Ident> {
	match pat {
		Pat::Ident(pat_ident) => {
			if let Some(by_ref) = pat_ident.by_ref {
				Err(Error::new(by_ref.span(), "`ref` not supported!"))
			} else if let Some(sub_pattern) = &pat_ident.subpat {
				Err(Error::new(sub_pattern.0.span(), "Not supported!"))
			} else {
				Ok(Ident::new(&format!("{}_ffi_value", pat_ident.ident), Span::call_site()))
			}
		}
		_ => Err(Error::new(pat.span(), "Not supported as variable name!"))
	}
}

/// Generate code that copies data from the host back to preallocated wasm memory.
///
/// Any argument that is given as `&mut` is interpreted as preallocated memory and it is expected
/// that the type implements `IntoPreAllocatedFFIValue`.
fn generate_into_preallocated_ffi_value(sig: &Signature) -> Result<TokenStream> {
	let crate_ = generate_crate_access();
	let ref_and_mut = get_function_argument_types_ref_and_mut(sig).map(|ram|
		ram.and_then(|(vr, vm)| vm.map(|v| (vr, v)))
	);
	let names_and_types = get_function_argument_names_and_types_without_ref(sig);

	ref_and_mut.zip(names_and_types)
		.filter_map(|(ram, (name, ty))| ram.map(|_| (name, ty)))
		.map(|(name, ty)| {
			let ffi_var_name = generate_ffi_value_var_name(name)?;

			Ok(
				quote! {
					<#ty as #crate_::host::IntoPreallocatedFFIValue>::into_preallocated_ffi_value(
						#name,
						context,
						#ffi_var_name,
					)?;
				}
			)
		})
		.collect()
}

/// Generate the code that converts the return value into the appropriate wasm value.
fn generate_return_value_into_wasm_value(sig: &Signature) -> TokenStream {
	let crate_ = generate_crate_access();

	match &sig.output {
		ReturnType::Default => quote!( Ok(None) ),
		ReturnType::Type(_, ty) => {
			let result_var_name = generate_host_function_result_var_name(&sig.ident);

			quote! {
				<#ty as #crate_::host::IntoFFIValue>::into_ffi_value(#result_var_name, context)
					.map(#crate_::wasm_interface::IntoValue::into_value)
					.map(Some)
			}
		}
	}
}