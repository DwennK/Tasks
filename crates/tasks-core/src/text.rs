//! A read-only visible-text projection. It neither modifies stored notes nor renders HTML.
use html5ever::tokenizer::{
    states::RawKind, BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer,
};
use pulldown_cmark::{Event, Options, Parser, TagEnd};
use std::cell::RefCell;

pub(super) fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn visible_notes(notes: &str, format: &str) -> String {
    match format {
        "html" => normalized(&html_text(notes)),
        "markdown" => {
            let mut text = String::new();
            for event in Parser::new_ext(notes, Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES)
            {
                match event {
                    Event::Text(value) | Event::Code(value) => text.push_str(&value),
                    Event::Html(value) | Event::InlineHtml(value) => {
                        text.push_str(&html_text(&value))
                    }
                    Event::SoftBreak | Event::HardBreak | Event::Rule => text.push(' '),
                    Event::End(
                        TagEnd::Paragraph
                        | TagEnd::Heading(_)
                        | TagEnd::Item
                        | TagEnd::BlockQuote(_)
                        | TagEnd::CodeBlock
                        | TagEnd::TableCell
                        | TagEnd::TableRow,
                    ) => text.push(' '),
                    _ => {}
                }
            }
            normalized(&text)
        }
        _ => notes.to_owned(),
    }
}

#[derive(Default)]
struct VisibleText {
    text: RefCell<String>,
    hidden: RefCell<Option<String>>,
}
impl TokenSink for VisibleText {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        match token {
            Token::CharacterTokens(text) if self.hidden.borrow().is_none() => {
                self.text.borrow_mut().push_str(&text);
            }
            Token::TagToken(tag) => {
                let name = tag.name.as_ref();
                if tag.kind == TagKind::EndTag {
                    if self.hidden.borrow().as_deref() == Some(name) {
                        *self.hidden.borrow_mut() = None;
                    }
                } else if matches!(name, "script" | "style") {
                    *self.hidden.borrow_mut() = Some(name.to_owned());
                    return TokenSinkResult::RawData(if name == "script" {
                        RawKind::ScriptData
                    } else {
                        RawKind::Rawtext
                    });
                } else if matches!(name, "textarea" | "title") {
                    return TokenSinkResult::RawData(RawKind::Rcdata);
                }
                if self.hidden.borrow().is_none()
                    && matches!(
                        name,
                        "address"
                            | "article"
                            | "aside"
                            | "blockquote"
                            | "br"
                            | "dd"
                            | "div"
                            | "dl"
                            | "dt"
                            | "figcaption"
                            | "figure"
                            | "footer"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "header"
                            | "hr"
                            | "li"
                            | "main"
                            | "nav"
                            | "ol"
                            | "p"
                            | "pre"
                            | "section"
                            | "table"
                            | "td"
                            | "th"
                            | "tr"
                            | "ul"
                    )
                {
                    self.text.borrow_mut().push(' ');
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

fn html_text(html: &str) -> String {
    let input = BufferQueue::default();
    input.push_back(html.into());
    let tokenizer = Tokenizer::new(VisibleText::default(), Default::default());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.text.into_inner()
}
