use syn::{
    Block, Generics, Ident, ItemFn, ReturnType, Signature, Token, Visibility,
    parse::ParseStream,
    punctuated::Punctuated,
};

fn gen_init_func(input: ParseStream) -> syn::Result<ItemFn> {
    input.parse::<Token![fn]>()?; // Consume 'fn'
    input.parse::<Token![$]>()?; // Consume '$'
    
    let init_ident: Ident = input.parse()?;
    if init_ident != "init" {
        return Err(syn::Error::new(
            init_ident.span(),
            "Expected 'init' after '$' in state impl block",
        ));
    }

    // Parse generics (e.g., <T, U: Clone>)
    let mut generics: Generics = input.parse()?;

    // Parse parameters in parentheses
    let content;
    syn::parenthesized!(content in input);
    let params = Punctuated::parse_terminated(&content)?;

    // Parse return type (-> T)
    let output: ReturnType = input.parse()?;

    // Parse where clause (e.g., where T: Display + Clone)
    generics.where_clause = input.parse()?;

    // Parse function body
    let body: Block = input.parse()?;

    Ok(ItemFn {
        attrs: Vec::new(),
        vis: Visibility::Inherited,
        sig: Signature {
            constness: None,
            asyncness: None,
            unsafety: None,
            abi: None,
            ident: init_ident.clone(),
            generics,
            inputs: params,
            output,
            variadic: None,
            fn_token: <Token![fn]>::default(),
            paren_token: syn::token::Paren(init_ident.span()),
        },
        block: Box::new(body),
    })
}

pub fn get_state_funcs(input: &mut ParseStream) -> syn::Result<(Vec<ItemFn>, Option<ItemFn>)> {
    input.parse::<Token![impl]>()?; // Consume 'impl'
    input.parse::<Token![$]>()?; // Consume '$'

    // consume 'state' identifier
    let state_ident: Ident = input.parse()?;
    if state_ident != "state" {
        return Err(syn::Error::new(
            state_ident.span(),
            "Expected 'state' after '$'",
        ));
    }

    let content;
    syn::braced!(content in input);

    let mut funcs = Vec::new();
    let mut init_func = None;

    while !content.is_empty() {
        if content.peek2(Token![$]) {
            // $init function
            init_func = Some(gen_init_func(&content)?);
        } else {
            // Regular function
            let func: ItemFn = content.parse()?;
            funcs.push(func);
        }
    }

    Ok((funcs, init_func))
}
