use syn::{
    Block, Generics, Ident, ItemFn, ReturnType, Signature, Token, Visibility,
    parse::{Parse, ParseBuffer, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

pub struct FuncData {
    pub code: ItemFn,
    pub reactive_args: Vec<Ident>,
    pub event_arg: Option<Ident>,
    pub is_event_handler: bool,
}

impl Parse for FuncData {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Parse visibility (pub, pub(crate), etc.)
        let vis: Visibility = input.parse()?;

        // Parse `fn` keyword
        input.parse::<Token![fn]>()?;

        // Parse function name
        let ident: Ident = input.parse()?;

        // Parse generics (e.g., <T, U: Clone>)
        let mut generics: Generics = input.parse()?;

        // Parse parameters in parentheses
        let content;
        syn::parenthesized!(content in input);
        let (params, reactive_args, event_arg, is_event_handler) = gen_params(content)?;

        // Parse return type (-> T)
        let output: ReturnType = input.parse()?;

        // Parse where clause (e.g., where T: Display + Clone)
        generics.where_clause = input.parse()?;

        // Parse function body
        let body: Block = input.parse()?;

        let code = ItemFn {
            attrs: Vec::new(),
            vis,
            sig: Signature {
                constness: None,
                asyncness: None,
                unsafety: None,
                abi: None,
                ident: ident.clone(),
                generics,
                inputs: params,
                output,
                variadic: None,
                fn_token: <Token![fn]>::default(),
                paren_token: syn::token::Paren(ident.span()),
            },
            block: Box::new(body),
        };

        Ok(FuncData {
            code,
            reactive_args,
            event_arg,
            is_event_handler,
        })
    }
}

pub fn gen_params(
    buffer: ParseBuffer,
) -> syn::Result<(
    Punctuated<syn::FnArg, syn::token::Comma>,
    Vec<Ident>,
    Option<Ident>,
    bool,
)> {
    let mut params = Vec::new();
    let mut reactive_args = Vec::new();
    while !buffer.is_empty() {
        // Check for reactive argument
        if buffer.parse::<Token![$]>().is_ok() {
            let ident = buffer.parse::<Ident>()?;
            reactive_args.push(ident.clone());
        } else {
            let param: syn::FnArg = buffer.parse()?;
            params.push(param);
        }

        if buffer.peek(Token![,]) {
            let _comma: Token![,] = buffer.parse()?;
        } else {
            break;
        }
    }

    let event_types = crate::EVENTS
        .iter()
        .map(|(_name, ty, _)| ty.to_string())
        .collect::<Vec<String>>();
    let mut event_arg_out = None;

    // Check for event argument
    for param in &params {
        if let syn::FnArg::Typed(pat_type) = param {
            let ty_str = quote::quote! { #pat_type.ty }.to_string();
            if event_types.contains(&ty_str) {
                if let Some(_) = event_arg_out {
                    return Err(syn::Error::new(
                        pat_type.span(),
                        "Multiple event arguments found; only one event argument is allowed per function",
                    ));
                }
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    event_arg_out = Some(pat_ident.ident.clone());
                }
            }
        }
    }

    let is_event_handler = params.len() == 0 || (event_arg_out.is_some() && params.len() == 1);

    Ok((
        Punctuated::from_iter(params),
        reactive_args,
        event_arg_out,
        is_event_handler,
    ))
}
