use tree_sitter::Language;

pub fn typescript() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

pub fn tsx() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}

pub fn python() -> Language {
    tree_sitter_python::LANGUAGE.into()
}

pub fn rust() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

pub fn go() -> Language {
    tree_sitter_go::LANGUAGE.into()
}

pub fn parse(lang: Language, source: &str) -> Option<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang).ok()?;
    parser.parse(source, None)
}
