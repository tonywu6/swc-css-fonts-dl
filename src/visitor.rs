use log::warn;
use relative_path::RelativePathBuf;
use swc_core::css::{
    self,
    ast::{Declaration, Ident, UrlValue},
    visit::{VisitMut, VisitMutWith},
};
use url::Url;

enum State {
    NoOp,
    AtFontFace,
    SrcDecl,
}

pub struct RemoteFont {
    pub url: Url,
    pub path: RelativePathBuf,
}

struct RewriteRemoteFonts<'output> {
    sources: &'output mut Vec<RemoteFont>,
    state: State,
    base: Option<Url>,
}

impl VisitMut for RewriteRemoteFonts<'_> {
    fn visit_mut_at_rule(&mut self, node: &mut css::ast::AtRule) {
        match node.name.as_ident() {
            Some(&Ident { ref value, .. }) if value == "font-face" => {
                self.state = State::AtFontFace;
                node.visit_mut_children_with(self);
                self.state = State::NoOp;
            }
            _ => {}
        }
    }

    fn visit_mut_declaration(&mut self, node: &mut Declaration) {
        match self.state {
            State::AtFontFace => {}
            _ => return,
        }

        match node.name.as_ident() {
            Some(&Ident { ref value, .. }) if value == "src" => {
                self.state = State::SrcDecl;
                node.visit_mut_children_with(self);
                self.state = State::AtFontFace;
            }
            _ => {}
        }
    }

    fn visit_mut_url_value(&mut self, node: &mut UrlValue) {
        match self.state {
            State::SrcDecl => {}
            _ => return,
        }
        self.rewrite_font_src(node);
    }
}

impl RewriteRemoteFonts<'_> {
    fn rewrite_font_src(&mut self, node: &mut UrlValue) -> Option<()> {
        let url_value = match node {
            UrlValue::Raw(url) => &url.value,
            UrlValue::Str(url) => &url.value,
        };

        let url_value = match Url::parse(&url_value) {
            Ok(parsed) => {
                if matches!(parsed.scheme(), "http" | "https") {
                    Some(parsed)
                } else {
                    None
                }
            }
            Err(err) => match err {
                url::ParseError::RelativeUrlWithoutBase => match self.base {
                    Some(ref base) => base.join(&url_value).ok(),
                    None => None,
                },
                _ => {
                    warn!("failed to parse url: {}", err);
                    None
                }
            },
        }?;

        let rel_path = std::iter::once(url_value.domain()?)
            .chain(url_value.path_segments()?)
            .fold(RelativePathBuf::new(), |acc, s| acc.join(s));

        let (node_value, node_raw) = match node {
            UrlValue::Raw(url) => (&mut url.value, &mut url.raw),
            UrlValue::Str(url) => (&mut url.value, &mut url.raw),
        };

        *node_value = format!("./{}", rel_path).into();
        *node_raw = None;

        self.sources.push(RemoteFont {
            url: url_value,
            path: rel_path,
        });

        Some(())
    }
}

pub fn rewrite_remote_fonts<'output>(
    urls: &'output mut Vec<RemoteFont>,
    base: Option<Url>,
) -> impl VisitMut + 'output {
    RewriteRemoteFonts {
        sources: urls,
        base: base,
        state: State::NoOp,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use swc_core::{
        common::{comments::SingleThreadedComments, sync::Lrc, FileName, SourceMap},
        css::{
            ast::Stylesheet,
            codegen::{
                writer::basic::{BasicCssWriter, BasicCssWriterConfig},
                CodeGenerator, CodegenConfig, Emit as _,
            },
            parser::parse_file,
            visit::VisitMutWith as _,
        },
    };
    use url::Url;

    use super::rewrite_remote_fonts;

    fn test_one(name: &str, base: Option<Url>) {
        let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);

        let sources: Lrc<SourceMap> = Default::default();
        let source = sources.new_source_file(
            FileName::Anon,
            std::fs::read_to_string(&source_path).unwrap(),
        );

        let comments = SingleThreadedComments::default();
        let mut css: Stylesheet =
            parse_file(&source, Some(&comments), Default::default(), &mut vec![]).unwrap();

        {
            let mut urls = vec![];
            let mut visitor = rewrite_remote_fonts(&mut urls, base);
            css.visit_mut_with(&mut visitor);
        }

        let mut output = String::new();
        output.push_str("``````css\n");
        {
            let writer = BasicCssWriter::new(&mut output, None, BasicCssWriterConfig::default());
            let mut generator = CodeGenerator::new(writer, CodegenConfig { minify: false });
            generator.emit(&css).unwrap();
        }
        output.push_str("\n``````\n");

        let snapshot_name = source_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let snapshot_path = source_path.parent().unwrap();

        insta::with_settings!({
            snapshot_path => snapshot_path,
            prepend_module_to_snapshot => false,
        }, {
            insta::assert_snapshot!(snapshot_name, output);
        });
    }

    #[test]
    fn test_default() {
        test_one("tests/fixtures/index.css", None);
    }

    #[test]
    fn test_same_origin_remote_font() {
        test_one(
            "tests/fixtures/same-origin.css",
            Some(Url::parse("https://rsms.me/inter/inter.css").unwrap()),
        );
    }
}
