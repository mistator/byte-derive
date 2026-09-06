mod utils;

extern crate core;

use proc_macro2::{Span, TokenStream};
use itertools::Itertools;
use quote::{ToTokens, quote};
use std::iter::zip;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{parse_macro_input, AngleBracketedGenericArguments, Attribute, Meta, PathArguments, Type, TypePath, Visibility, GenericArgument};
use crate::utils::{bool_expr, is_primitive};

#[proc_macro_derive(TryRead, attributes(byte))]
pub fn try_read_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast = parse_macro_input!(input as ObjectDef);
    let mut stream = TokenStream::new();
    ast.try_read_to_tokens(&mut stream);
    stream.into()
}

#[proc_macro_derive(TryWrite, attributes(byte))]
pub fn try_write_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast = parse_macro_input!(input as ObjectDef);
    let mut stream = TokenStream::new();
    ast.try_write_to_tokens(&mut stream);
    stream.into()
}

#[allow(clippy::large_enum_variant)]
enum ObjectDef {
    Struct(StructDef),
    Enum(EnumDef),
}

impl Parse for ObjectDef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let clone = input.fork();

        clone.call(Attribute::parse_outer)?;
        clone.parse::<Visibility>()?;

        if clone.peek(syn::Token![struct]) {
            Ok(ObjectDef::Struct(StructDef::parse(input)?))
        } else if clone.peek(syn::Token![enum]) {
            Ok(ObjectDef::Enum(EnumDef::parse(input)?))
        } else {
            Err(syn::Error::new(input.span(), "not enum or struct"))
        }
    }
}

impl ObjectDef {
    fn try_read_to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            ObjectDef::Struct(strct) => strct.try_read_to_tokens(tokens),
            ObjectDef::Enum(enm) => enm.try_read_to_tokens(tokens),
        }
    }

    fn try_write_to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            ObjectDef::Struct(strct) => strct.try_write_to_tokens(tokens),
            ObjectDef::Enum(enm) => enm.try_write_to_tokens(tokens),
        }
    }
}

struct EnumDef {
    pub ctx: Option<syn::Expr>,
    pub ident: syn::Ident,
    pub repr_ty: syn::Type,
    pub generics: syn::Generics,
    pub variants: Vec<EnumVariant>,
    pub no_tag: bool,
}

impl Parse for EnumDef {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let enm = input.parse::<syn::ItemEnum>()?;

        let repr_attr = enm
            .attrs
            .iter()
            .find(|attr| attr.meta.path().is_ident("repr"))
            .ok_or(syn::Error::new(enm.span(), "repr attribute not found"))?;

        let repr_ty = match &repr_attr.meta {
            Meta::List(list) => list.parse_args_with(syn::Type::parse),
            _ => Err(syn::Error::new(
                repr_attr.span(),
                "repr attribute must be a list",
            )),
        }?;

        let variants = enm
            .variants
            .iter()
            .map(|var| {
                let ident = var.ident.clone();
                let discriminant = var.discriminant.clone().unwrap().1;

                match &var.fields {
                    syn::Fields::Named(named) => {
                        let fields = Some(Fields::Named(
                            named
                                .named
                                .iter()
                                .map(Field::from_named)
                                .collect_vec(),
                        ));

                        EnumVariant {
                            ident,
                            fields,
                            discriminant,
                        }
                    }
                    syn::Fields::Unnamed(unnamed) => {
                        let fields = Some(Fields::Unnamed(
                            unnamed
                                .unnamed
                                .iter()
                                .enumerate()
                                .map(|(idx, field)| Field::from_unnamed(field, idx))
                                .collect_vec(),
                        ));

                        EnumVariant {
                            ident,
                            fields,
                            discriminant,
                        }
                    }
                    syn::Fields::Unit => EnumVariant {
                        ident,
                        discriminant,
                        fields: None,
                    },
                }
            })
            .collect_vec();

        let attr = enm
            .attrs
            .iter()
            .find(|attr| attr.meta.path().is_ident("byte"));

        let mut ctx = None;
        let mut no_tag = false;
        if let Some(attr) = attr
            && let syn::Meta::List(ref meta_list) = attr.meta {
                let parser = Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated;
                let args = meta_list.parse_args_with(parser)?;

                for arg in args {
                    match get_path_name(&arg.path).as_str() {
                        "ctx" => ctx = Some(arg.value),
                        "no_tag" => no_tag = bool_expr(&arg.value)?,
                        _ => {
                            return Err(syn::Error::new_spanned(arg, "invalid attribute"));
                        }
                    }
                }
            }

        Ok(Self {
            ctx,
            ident: enm.ident,
            repr_ty,
            generics: enm.generics,
            variants,
            no_tag,
        })
    }
}

impl EnumDef {
    fn try_read_to_tokens(&self, tokens: &mut TokenStream) {
        let generics = &self.generics.params;
        let ident = &self.ident;
        let repr_ty = &self.repr_ty;

        let mut inner = TokenStream::new();
        for variant in &self.variants {
            variant.try_read_to_tokens(&mut inner);
        }

        let ctx = if let Some(ctx) = self.ctx.as_ref() {
            quote!(#ctx)
        } else {
            quote!(::byte::ctx::Endian)
        };

        tokens.extend(quote! {
            impl<#generics> ::byte::TryRead<'_, #ctx> for #ident<#generics> {
                fn try_read(bytes: &'_ [u8], ctx: #ctx) -> ::byte::Result<(Self, usize)> {
                    use ::byte::BytesExt;
                    let offset = &mut 0;

                    let discriminant = bytes.read_with::<#repr_ty>(offset, byte::LE)?;
                    let value = match discriminant {
                        #inner
                        _ => {
                            return Err(byte::Error::BadInput { err: "unknown enum tag received" })
                        }
                    };

                    Ok((value, *offset))
                }
            }

            impl<#generics> ::byte::TryRead<'_, #repr_ty> for #ident<#generics> {
                fn try_read(bytes: &'_ [u8], discriminant: #repr_ty) -> ::byte::Result<(Self, usize)> {
                    use ::byte::BytesExt;
                    let offset = &mut 0;

                    let ctx = byte::LE;
                    let value = match discriminant {
                        #inner
                        _ => {
                            return Err(byte::Error::BadInput { err: "unknown enum tag received" })
                        }
                    };

                    Ok((value, *offset))
                }
            }
        });
    }

    fn try_write_to_tokens(&self, tokens: &mut TokenStream) {
        let generics = &self.generics.params;
        let ident = &self.ident;
        let _repr_ty = &self.repr_ty;

        let mut inner = TokenStream::new();
        for variant in &self.variants {
            variant.try_write_to_tokens(&mut inner, &self.repr_ty, self.no_tag);
        }

        let ctx = if let Some(ctx) = self.ctx.as_ref() {
            quote!(#ctx)
        } else {
            quote!(::byte::ctx::Endian)
        };

        tokens.extend(quote! {
            impl<#generics> ::byte::TryWrite<#ctx> for #ident<#generics> {
                fn try_write(self, bytes: &mut [u8], ctx: #ctx) -> ::byte::Result<usize> {
                    use ::byte::BytesExt;
                    let offset = &mut 0;

                    match self {
                        #inner
                    }

                    Ok(*offset)
                }
            }
        });
    }
}

struct EnumVariant {
    ident: syn::Ident,
    discriminant: syn::Expr,
    fields: Option<Fields>,
}

impl EnumVariant {
    fn make_ctor(&self) -> TokenStream {
        let ident = &self.ident;

        match &self.fields {
            Some(fields) => {
                let idents = fields
                    .get_fields()
                    .iter()
                    .map(|field| field.get_ident())
                    .collect_vec();

                match fields {
                    Fields::Named(_) => quote!(Self::#ident { #(#idents),* }),
                    Fields::Unnamed(_) => quote!(Self::#ident ( #(#idents),* )),
                }
            }
            None => {
                let ident = &self.ident;
                quote!(Self::#ident)
            }
        }
    }

    fn make_try_read_block(&self) -> TokenStream {
        match &self.fields {
            Some(fields) => {
                let mut block = TokenStream::new();

                for field in fields.get_fields() {
                    field.try_read_to_tokens(&mut block);
                }

                let ctor = self.make_ctor();
                quote! {
                    #block
                    #ctor
                }
            }
            None => self.make_ctor(),
        }
    }

    fn make_try_write_block(&self) -> TokenStream {
        match &self.fields {
            Some(Fields::Unnamed(fields)) => {
                let mut block = TokenStream::new();

                for field in fields.iter() {
                    field.try_write_to_tokens(&mut block, false);
                }

                quote!(#block)
            }
            Some(Fields::Named(fields)) => {
                let mut block = TokenStream::new();

                for field in fields.iter() {
                    field.try_write_to_tokens(&mut block, false);
                }

                quote!(#block)
            }
            None => quote!(),
        }
    }

    fn try_read_to_tokens(&self, tokens: &mut TokenStream) {
        let discriminant = &self.discriminant;
        let try_read_block = self.make_try_read_block();

        tokens.extend(quote! {
            #discriminant => { #try_read_block },
        });
    }

    fn try_write_to_tokens(&self, tokens: &mut TokenStream, repr_ty: &syn::Type, no_tag: bool) {
        let try_write_block = self.make_try_write_block();
        let ctor = self.make_ctor();
        let discriminant = &self.discriminant;

        if no_tag {
            tokens.extend(quote! {
                #ctor => {
                    #try_write_block
                }
            });
        } else {
            tokens.extend(quote! {
                #ctor => {
                    bytes.write_with(offset, #discriminant as #repr_ty, byte::LE)?;
                    #try_write_block
                }
            });
        }
    }
}

struct StructDef {
    ctx: Option<syn::Expr>,
    pub ident: syn::Ident,
    pub generics: syn::Generics,
    pub fields: Option<Fields>,
}

impl Parse for StructDef {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let strct = input.parse::<syn::ItemStruct>()?;

        let fields = match strct.fields {
            syn::Fields::Named(named) => Fields::Named(
                named
                    .named
                    .iter()
                    .map(Field::from_named)
                    .collect_vec(),
            )
            .into(),
            syn::Fields::Unnamed(unnamed) => Fields::Unnamed(
                unnamed
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(idx, field)| Field::from_unnamed(field, idx))
                    .collect_vec(),
            )
            .into(),
            syn::Fields::Unit => None,
        };

        let mut ctx = None;
        for attr in strct.attrs {
            if let syn::Meta::List(ref meta_list) = attr.meta {
                let parser = Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated;
                let args = meta_list.parse_args_with(parser)?;

                for arg in args {
                    match get_path_name(&arg.path).as_str() {
                        "ctx" => ctx = Some(arg.value),
                        _ => {
                            return Err(syn::Error::new_spanned(arg, "invalid attribute"));
                        }
                    }
                }
            }
        }

        Ok(Self {
            ctx,
            ident: strct.ident,
            generics: strct.generics,
            fields,
        })
    }
}

impl StructDef {
    fn try_read_to_tokens(&self, tokens: &mut TokenStream) {
        let generics = &self.generics.params;
        let ident = &self.ident;

        let mut try_read_parse = TokenStream::new();
        let mut self_obj = TokenStream::new();

        match &self.fields {
            None => self_obj.extend(quote! { Self }),
            Some(fields) => {
                let mut self_fields = TokenStream::new();

                for field in fields.get_fields() {
                    let ident = &field.get_ident();
                    field.try_read_to_tokens(&mut try_read_parse);
                    self_fields.extend(quote! { #ident, });
                }

                match fields {
                    Fields::Named(_) => {
                        self_obj.extend(quote! {
                            Self { #self_fields }
                        });
                    }
                    Fields::Unnamed(_) => {
                        self_obj.extend(quote! {
                            Self( #self_fields )
                        });
                    }
                }
            }
        };

        let ctx = if let Some(ctx) = self.ctx.as_ref() {
            quote!(#ctx)
        } else {
            quote!(::byte::ctx::Endian)
        };

        tokens.extend(quote! {
            impl<#generics> ::byte::TryRead<'_, #ctx> for #ident<#generics> {
                fn try_read(bytes: &'_ [u8], ctx: #ctx) -> ::byte::Result<(Self, usize)> {
                    use ::byte::BytesExt;

                    let offset = &mut 0;
                    #try_read_parse

                    Ok((#self_obj, *offset))
                }
            }
        });
    }

    fn try_write_to_tokens(&self, tokens: &mut TokenStream) {
        let generics = &self.generics.params;
        let ident = &self.ident;

        let mut try_write_parse = TokenStream::new();
        match &self.fields {
            None => {}
            Some(fields) => {
                for field in fields.get_fields() {
                    field.try_write_to_tokens(&mut try_write_parse, true);
                }
            }
        }

        let ctx = if let Some(ctx) = self.ctx.as_ref() {
            quote!(#ctx)
        } else {
            quote!(::byte::ctx::Endian)
        };

        tokens.extend(quote! {
            impl<#generics> ::byte::TryWrite<#ctx> for & #ident<#generics> {
                fn try_write(self, bytes: &mut [u8], ctx: #ctx) -> ::byte::Result<usize> {
                    use ::byte::BytesExt;

                    let offset = &mut 0;
                    #try_write_parse

                    Ok(*offset)
                }
            }

            impl<#generics> ::byte::TryWrite<#ctx> for #ident<#generics> {
                fn try_write(self, bytes: &mut [u8], ctx: #ctx) -> ::byte::Result<usize> {
                    (&self).try_write(bytes, ctx)
                }
            }
        });
    }
}

enum Fields {
    Named(Vec<Field>),
    Unnamed(Vec<Field>),
}

impl Fields {
    fn get_fields(&self) -> &Vec<Field> {
        match self {
            Fields::Named(fields) => fields,
            Fields::Unnamed(fields) => fields,
        }
    }
}

enum FieldDef {
    Named(syn::Ident),
    Unnamed(usize),
}

struct Field {
    pub config: FieldConfig,
    pub ty: syn::Type,
    pub field_def: FieldDef,
}

impl Field {
    fn from_named(named: &syn::Field) -> Field {
        Field {
            config: FieldConfig::from_attrs(&named.attrs).unwrap(),
            ty: named.ty.clone(),
            field_def: FieldDef::Named(named.ident.clone().unwrap()),
        }
    }

    fn from_unnamed(unnamed: &syn::Field, idx: usize) -> Field {
        Field {
            config: FieldConfig::from_attrs(&unnamed.attrs).unwrap(),
            ty: unnamed.ty.clone(),
            field_def: FieldDef::Unnamed(idx),
        }
    }

    fn get_ident(&self) -> TokenStream {
        match self.field_def {
            FieldDef::Named(ref ident) => quote!(#ident),
            FieldDef::Unnamed(idx) => {
                let ident =
                    syn::Ident::new(&format!("field_{}", idx), Span::call_site());
                quote!(#ident)
            }
        }
    }

    fn get_self_call(&self) -> TokenStream {
        match self.field_def {
            FieldDef::Named(ref ident) => quote!(self.#ident),
            FieldDef::Unnamed(idx) => {
                let idx = syn::Index::from(idx);
                quote!(self.#idx)
            }
        }
    }

    fn try_read_to_tokens(&self, tokens: &mut TokenStream) {
        let ident = self.get_ident();
        if self.config.ignore {
            match self.config.default {
                None => tokens.extend(quote!(let #ident = Default::default();)),
                Some(ref expr) => tokens.extend(quote!(let #ident = #expr;)),
            }
            return;
        }

        let ctx = self.config.get_ctx(&self.ty);

        let qt = match &self.ty {
            Type::Array(ty) => {
                let len = ty.len.to_token_stream();
                let inner_ty = ty.elem.to_token_stream();

                quote! {
                    let mut #ident = [#inner_ty::default(); #len];

                    for i in 0..#len {
                        #ident[i] = bytes.read_with::<#inner_ty>(offset, #ctx)?;
                    }
                }
            }
            Type::FnPtr(_ty) => {
                panic!("fnptr")
            }
            Type::Group(_ty) => {
                panic!("group")
            }
            Type::ImplTrait(_ty) => {
                panic!("impl")
            }
            Type::Infer(_ty) => {
                panic!("infer")
            }
            Type::Macro(_ty) => {
                panic!("macro")
            }
            Type::Never(_ty) => {
                panic!("never")
            }
            Type::Paren(_ty) => {
                panic!("paren")
            }
            Type::Path(ty) => {
                if path_ends_with(&ty.path, "Option") {
                    self.read_option(&ty.path)
                } else if path_ends_with(&ty.path, "Vec") {
                    self.read_vec(&ty.path)
                } else {
                    quote!(let #ident = bytes.read_with::<#ty>(offset, #ctx)?;)
                }
            }
            Type::Ptr(_ty) => {
                panic!("ptr")
            }
            Type::Reference(_ty) => {
                panic!("reference")
            }
            Type::Slice(_ty) => {
                panic!("slice")
            }
            Type::TraitObject(_ty) => {
                panic!("trait_object")
            }
            Type::Tuple(ty) => {
                let mut stream = TokenStream::new();
                for inner_ty in ty.elems.iter() {
                    stream.extend(quote!(bytes.read_with::<#inner_ty>(offset, #ctx)?,));
                }

                quote! {
                    let #ident = (
                        #stream
                    );
                }
            }
            Type::Verbatim(_ty) => {
                panic!("verbatim")
            }
            _ => panic!("unsupported type"),
        };
        tokens.extend(qt);
    }

    fn read_option(&self, path: &syn::Path) -> TokenStream {
        let mut inner_ty = TokenStream::new();
        type_generics_to_tokens(path, &mut inner_ty);

        let ident = self.get_ident();
        let ctx = self.config.get_ctx(&self.ty);

        match self.config.parse_if {
            Some(ref expr) => {
                quote!{
                    let #ident = if #expr {
                        Some(bytes.read_with::<#inner_ty>(offset, #ctx)?)
                    } else {
                        None
                    };
                }
            }
            None => quote!{
                let is_some = bytes.read_with::<bool>(offset, ())?;
                let #ident = if is_some {
                    Some(bytes.read_with::<#inner_ty>(offset, #ctx)?)
                } else {
                    None
                };
            }
        }
    }

    fn read_vec(&self, path: &syn::Path) -> TokenStream {
        let ident = self.get_ident();
        let ctx = self.config.get_ctx(&self.ty);
        let len = self.config.get_len();

        let (_, size) = match &path.segments.last().unwrap().arguments {
            PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) => {
                let items_ty = args.first().to_token_stream();
                let size = if args.len() == 2 {
                    Some(args[1].to_token_stream())
                } else {
                    None
                };

                (items_ty, size)
            }
            _ => {
                panic!()
            }
        };

        let mut vec_type = TokenStream::new();
        let mut is_first = true;
        for segment in path.segments.iter() {
            if !is_first {
                let tok = syn::Token![::](segment.ident.span());
                vec_type.extend(quote! {#tok});
            }
            is_first = false;
            vec_type.extend(segment.ident.to_token_stream());

            if !matches!(segment.arguments, PathArguments::None) {
                let tok = syn::Token![::](segment.ident.span());
                vec_type.extend(quote! {#tok});
                vec_type.extend(segment.arguments.to_token_stream());
            }
        }

        let ctor = if let Some(ref expr) = len {
            if size.is_none() {
                quote! {
                    let size = bytes.read_with::<#expr>(offset, #ctx)?;
                    let mut #ident = #vec_type::with_capacity(size as usize);
                }
            } else {
                quote! {
                    let size = bytes.read_with::<#expr>(offset, #ctx)?;
                    let mut #ident = #vec_type::new();
                }
            }
        } else {
            quote! {
                let mut #ident = #vec_type::new();
            }
        };

        let inner = if size.is_some() {
            quote! {
                #ident.push(bytes.read_with(offset, #ctx)?)
                    .map_err(|_| byte::Error::BadInput {
                        err: "cannot fit items into Vec, Vec too small"
                    })?;
            }
        } else {
            quote! {
                #ident.push(bytes.read_with(offset, #ctx)?);
            }
        };

        if len.is_some() {
            quote! {
                #ctor
                for _ in 0..size {
                    #inner
                }
            }
        } else {
            quote! {
                #ctor
                while bytes[*offset..].len() > 0 {
                    #inner
                }
            }
        }
    }

    fn write_field(&self, field: &TokenStream, ctx: &TokenStream) -> TokenStream {
        if is_primitive(&self.ty) {
            quote! { bytes.write_with(offset, #field, #ctx)?; }
        } else {
            quote! { bytes.write_with(offset, &#field, #ctx)?; }
        }
    }



    fn try_write_to_tokens(&self, tokens: &mut TokenStream, self_ref: bool) {
        if self.config.ignore {
            return;
        }

        let slf = if self_ref {
            self.get_self_call()
        } else {
            self.get_ident()
        };
        let ctx = self.config.get_ctx(&self.ty);

        let qt = match &self.ty {
            Type::Array(ty) => {
                let len = ty.len.to_token_stream();
                let write_field = if is_primitive(&*ty.elem) {
                    quote! { bytes.write_with(offset, #slf[i], #ctx)?; }
                } else {
                    quote! { bytes.write_with(offset, &#slf[i], #ctx)?; }
                };

                quote! {
                    for i in 0..#len {
                        #write_field
                    }
                }
            }
            Type::Path(ty) => {
                if path_ends_with(&ty.path, "Option") {
                    self.write_option(&ty, &slf)
                } else if path_ends_with(&ty.path, "Vec") {
                    self.write_vec(&ty, &slf)
                } else {
                    self.write_field(&slf, &ctx)
                }
            }
            Type::Tuple(ty) => {
                let len = ty.elems.len();
                let mut stream = TokenStream::new();

                for i in 0..len {
                    let idx = syn::Index::from(i);
                    let item_ty = ty.elems[i].clone();

                    if is_primitive(&item_ty) {
                        stream.extend(quote! { bytes.write_with(offset, #slf.#idx, #ctx)?; } );
                    } else {
                        stream.extend(quote! { bytes.write_with(offset, &#slf.#idx, #ctx)?; } );
                    }
                }

                quote! {
                    #stream
                }
            }
            &_ => panic!("unsupported type"),
        };
        tokens.extend(qt);
    }

    fn write_option(&self, ty: &TypePath, slf: &TokenStream) -> TokenStream {
        let ty = match &ty.path.segments.last().unwrap().arguments {
            PathArguments::AngleBracketed(generics) => {
                match generics.args.first().unwrap() {
                    GenericArgument::Type(ty) => ty,
                    _ => panic!("unsupported generic type for option")
                }
            }
            _ => unreachable!(),
        };
        let ctx = self.config.get_ctx(&self.ty);

        let write_expr = if is_primitive(ty) {
            quote! { bytes.write_with(offset, *value, #ctx)?; }
        } else {
            quote! { bytes.write_with(offset, value, #ctx)?; }
        };

        match self.config.parse_if {
            Some(_) => quote! {
                if let Some(ref value) = #slf {
                    #write_expr
                }
            },
            None => quote! {
                if let Some(ref value) = #slf {
                    bytes.write_with(offset, true, ())?;
                    #write_expr
                } else {
                    bytes.write_with(offset, false, ())?;
                }
            }
        }
    }

    fn write_vec(&self, ty: &TypePath, slf: &TokenStream) -> TokenStream {
        let ctx = self.config.get_ctx(&self.ty);
        let len = self.config.get_len();

        let ty = match &ty.path.segments.last().unwrap().arguments {
            PathArguments::AngleBracketed(generics) => {
                match generics.args.first().unwrap() {
                    GenericArgument::Type(ty) => ty,
                    _ => panic!("unsupported generic type for option")
                }
            }
            _ => unreachable!(),
        };

        let write_block = if is_primitive(&ty) {
            quote!(bytes.write_with(offset, *item, #ctx)?;)
        } else  {
            quote!(bytes.write_with(offset, item, #ctx)?;)
        };

        let mut block = TokenStream::new();

        if let Some(expr) = len {
            block.extend(quote! {
                bytes.write_with::<#expr>(offset, #slf.len() as #expr, #ctx)?;
            });
        }

        block.extend(quote! {
            for item in #slf.iter() {
                #write_block
            }
        });

        block
    }
}

#[derive(Clone, Default)]
struct FieldConfig {
    ctx: Option<syn::Expr>,
    ctx_write: Option<syn::Expr>,
    parse_if: Option<syn::Expr>,
    len: Option<syn::Expr>,
    ignore: bool,
    default: Option<syn::Expr>,
}

impl FieldConfig {
    fn get_ctx(&self, ty: &syn::Type) -> TokenStream {
        match &self.ctx {
            Some(ctx) => quote!(#ctx),
            None => {
                if let syn::Type::Path(syn::TypePath { path, .. }) = ty
                    && path.segments.len() == 1 && path.segments[0].ident == "bool" {
                        return quote!(());
                    }

                quote!(ctx)
            }
        }
    }

    fn get_len(&self) -> Option<TokenStream> {
        self.len.as_ref().map(|expr| quote!(#expr))
    }
}

impl FieldConfig {
    fn from_attrs(attrs: &Vec<syn::Attribute>) -> syn::Result<Self> {
        let mut slf = Self::default();

        let Some(attr) = attrs.iter().find(|attr| attr.meta.path().is_ident("byte")) else {
            return Ok(slf)
        };

        if let syn::Meta::List(ref meta_list) = attr.meta {
            let parser = Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated;
            let args = meta_list.parse_args_with(parser)?;

            for arg in args {
                match get_path_name(&arg.path).as_str() {
                    "ctx" => slf.ctx = Some(arg.value),
                    "ctx_write" => slf.ctx_write = Some(arg.value),
                    "parse_if" => slf.parse_if = Some(arg.value),
                    "len" => slf.len = Some(arg.value),
                    "ignore" => slf.ignore = bool_expr(&arg.value)?,
                    "default" => slf.default = Some(arg.value),
                    _ => {
                        return Err(syn::Error::new_spanned(arg, "invalid attribute"));
                    }
                }
            }
        }

        Ok(slf)
    }
}

fn get_path_name(path: &syn::Path) -> String {
    if path.segments.len() != 1 {
        String::new()
    } else {
        path.segments[0].ident.to_string()
    }
}

fn path_ends_with(path: &syn::Path, ident_str: &'static str) -> bool {
    let segments = &path.segments;

    let parts = ident_str.split("::").collect::<Vec<_>>();
    if path.segments.len() < parts.len() {
        return false;
    }

    for (segment, part) in zip(segments.iter().skip(segments.len() - parts.len()), parts) {
        if *segment.ident.to_string().as_str() != *part {
            return false;
        }
    }

    true
}

fn type_generics_to_tokens(path: &syn::Path, tokens: &mut TokenStream) {
    let segment = path.segments.last().unwrap();
    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
        let args = &args.args;
        tokens.extend(quote! {#args})
    }
}
