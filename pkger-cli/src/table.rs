#![allow(dead_code)]
use colored::{Color, Colorize};

pub mod style {
    #[derive(Copy, Clone, Debug, Default, PartialEq)]
    pub struct Style(u8);

    impl From<u8> for Style {
        fn from(style: u8) -> Self {
            Self(style)
        }
    }

    impl Style {
        pub fn add_style(mut self, style: u8) -> Self {
            self.0 |= style;
            self
        }

        pub fn is_bold(&self) -> bool {
            self.0 & BOLD != 0
        }

        pub fn is_italic(&self) -> bool {
            self.0 & ITALIC != 0
        }

        pub fn is_underline(&self) -> bool {
            self.0 & UNDERLINE != 0
        }

        pub fn is_reversed(&self) -> bool {
            self.0 & REVERSED != 0
        }
    }

    pub const BOLD: u8 = 0b0000_0001;
    pub const UNDERLINE: u8 = 0b0000_0010;
    pub const REVERSED: u8 = 0b0000_0100;
    pub const ITALIC: u8 = 0b0000_1000;

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn adds_styles() {
            let style = Style::default().add_style(BOLD).add_style(ITALIC);

            assert!(style.is_bold());
            assert!(style.is_italic());
            assert!(!style.is_reversed());
            assert!(!style.is_underline());

            let style = Style::default().add_style(UNDERLINE).add_style(REVERSED);

            assert!(!style.is_bold());
            assert!(!style.is_italic());
            assert!(style.is_reversed());
            assert!(style.is_underline());
        }
    }
}

use style::Style;

#[derive(Debug, PartialEq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

#[derive(Debug)]
pub struct Cell {
    text: String,
    alignment: Alignment,
    color: Color,
    style: Style,
}

impl Cell {
    pub fn new<T: Into<String>>(text: T) -> Self {
        Self {
            text: text.into(),
            alignment: Alignment::Center,
            color: Color::BrightWhite,
            style: Style::default(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn left(mut self) -> Self {
        self.alignment = Alignment::Left;
        self
    }

    pub fn right(mut self) -> Self {
        self.alignment = Alignment::Right;
        self
    }

    pub fn center(mut self) -> Self {
        self.alignment = Alignment::Center;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn add_style(mut self, style: u8) -> Self {
        self.style = self.style.add_style(style);
        self
    }

    pub fn bold(self) -> Self {
        self.add_style(style::BOLD)
    }

    pub fn italic(self) -> Self {
        self.add_style(style::ITALIC)
    }
    pub fn underline(self) -> Self {
        self.add_style(style::UNDERLINE)
    }

    pub fn reversed(self) -> Self {
        self.add_style(style::REVERSED)
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

pub trait IntoCell {
    fn cell(self) -> Cell;
}

impl IntoCell for &str {
    fn cell(self) -> Cell {
        Cell::from(self)
    }
}

impl IntoCell for String {
    fn cell(self) -> Cell {
        Cell::from(self)
    }
}

#[derive(Debug)]
enum Token<'text> {
    Text(&'text str, Color, Style),
    Padding(usize),
    ColumnSeparator,
    RowSeparator(usize),
    NewLine,
}

#[derive(Debug)]
pub struct Table {
    rows: Vec<Vec<Cell>>,
    headers: Vec<Cell>,
    col_separator: char,
    row_separator: Option<char>,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            rows: vec![],
            headers: vec![],
            col_separator: ' ',
            row_separator: None,
        }
    }
}

impl Table {
    pub fn with_column_separator(mut self, separator: char) -> Self {
        self.col_separator = separator;
        self
    }

    pub fn with_row_separator(mut self, separator: char) -> Self {
        self.row_separator = Some(separator);
        self
    }

    pub fn with_headers<H, I>(mut self, headers: I) -> Self
    where
        H: Into<Cell>,
        I: IntoIterator<Item = H>,
    {
        self.headers = headers.into_iter().map(H::into).collect();
        self
    }

    pub fn with_header_cells<H>(mut self, headers: H) -> Self
    where
        H: IntoIterator<Item = Cell>,
    {
        self.headers = headers.into_iter().collect();
        self
    }

    pub fn push_row(&mut self, row: Vec<Cell>) {
        self.rows.push(row);
    }

    fn tokenize(&self) -> impl Iterator<Item = Token> {
        let mut tokens = vec![];

        macro_rules! add_text_with_padding {
            ($text:ident, $cell:expr, $padding:expr, $is_last_col:expr) => {
                match $cell.alignment {
                    Alignment::Left => {
                        tokens.push(Token::Text($text, $cell.color, $cell.style));
                        if !$is_last_col {
                            tokens.push(Token::Padding($padding));
                        }
                    }
                    Alignment::Center => {
                        let new_padding = (($padding as f64) / 2.).floor() as usize;
                        tokens.push(Token::Padding(new_padding));
                        tokens.push(Token::Text($text, $cell.color, $cell.style));
                        if !$is_last_col {
                            tokens.push(Token::Padding(new_padding));
                            if $padding % 2 != 0 {
                                tokens.push(Token::Padding(1));
                            }
                        }
                    }
                    Alignment::Right => {
                        tokens.push(Token::Padding($padding));
                        tokens.push(Token::Text($text, $cell.color, Style::from($cell.style)));
                    }
                }
            };
        }

        let n_cols = {
            let mut n_cols = 0;
            for row in &self.rows {
                let n = row.len();
                if n > n_cols {
                    n_cols = n;
                }
            }
            n_cols
        };
        let mut cols_max = vec![0usize; n_cols];
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                cols_max[i] = usize::max(cols_max[i], cell.text().len());
            }
        }

        if !self.headers.is_empty() {
            let headers_last = self.headers.len() - 1;
            for (i, header) in self.headers.iter().enumerate() {
                let text = header.text();
                let len = text.len();
                if i > cols_max.len() {
                    cols_max.push(len);
                } else {
                    cols_max[i] = usize::max(cols_max[i], len)
                }

                let padding = cols_max[i].saturating_sub(len);

                add_text_with_padding!(text, &header, padding, i == headers_last);

                if i != headers_last {
                    tokens.push(Token::ColumnSeparator);
                }
            }

            tokens.push(Token::NewLine);
        }

        let total_width = cols_max.iter().fold(0usize, |acc, col| acc + col + 1);
        if self.row_separator.is_some() {
            tokens.push(Token::RowSeparator(total_width));
            tokens.push(Token::NewLine);
        }

        let cols_max_len = cols_max.len();

        for row in self.rows.iter() {
            if !row.is_empty() {
                let last_col = row.len() - 1;
                for (i, (cell, col_size)) in row.iter().zip(cols_max.iter()).enumerate() {
                    let text = cell.text();
                    let padding = col_size.saturating_sub(text.len());

                    add_text_with_padding!(text, &cell, padding, i == cols_max_len - 1);

                    if i != last_col {
                        tokens.push(Token::ColumnSeparator);
                    }
                }
                if last_col + 1 < cols_max_len {
                    tokens.push(Token::ColumnSeparator);

                    for (i, &col_size) in cols_max[last_col + 1..cols_max_len].iter().enumerate() {
                        tokens.push(Token::Padding(col_size));

                        if i + last_col + 1 != cols_max_len - 1 {
                            tokens.push(Token::ColumnSeparator);
                        }
                    }
                }
            } else {
                for (i, &col_size) in cols_max.iter().enumerate() {
                    tokens.push(Token::Padding(col_size));

                    if i != cols_max_len - 1 {
                        tokens.push(Token::ColumnSeparator);
                    }
                }
            }
            tokens.push(Token::NewLine);
            if self.row_separator.is_some() {
                tokens.push(Token::RowSeparator(total_width));
                tokens.push(Token::NewLine);
            }
        }

        tokens.into_iter()
    }

    pub fn render(&self, add_color: bool) -> String {
        let mut s = String::new();
        let mut tokens = self.tokenize();

        loop {
            match tokens.next() {
                Some(Token::Text(text, color, style)) => {
                    if add_color {
                        let mut text = text.color(color);
                        if style.is_bold() {
                            text = text.bold();
                        }
                        if style.is_reversed() {
                            text = text.reverse();
                        }
                        if style.is_italic() {
                            text = text.italic();
                        }
                        if style.is_underline() {
                            text = text.underline();
                        }
                        s.push_str(text.color(color).as_ref());
                    } else {
                        s.push_str(text);
                    }
                }
                Some(Token::NewLine) => s.push('\n'),
                Some(Token::ColumnSeparator) => s.push(self.col_separator),
                Some(Token::RowSeparator(n)) => {
                    for _ in 0..n {
                        s.push(self.row_separator.unwrap_or_default());
                    }
                }
                Some(Token::Padding(n)) => {
                    for _ in 0..n {
                        s.push(' ');
                    }
                }
                None => break,
            }
        }

        s
    }

    pub fn print(&self) {
        let mut tokens = self.tokenize();

        loop {
            match tokens.next() {
                Some(Token::Text(text, color, style)) => {
                    let mut text = text.color(color);
                    if style.is_bold() {
                        text = text.bold();
                    }
                    if style.is_reversed() {
                        text = text.reverse();
                    }
                    if style.is_italic() {
                        text = text.italic();
                    }
                    if style.is_underline() {
                        text = text.underline();
                    }
                    print!("{}", text)
                }
                Some(Token::NewLine) => println!(),
                Some(Token::ColumnSeparator) => print!("{}", self.col_separator),
                Some(Token::RowSeparator(n)) => {
                    let separator = self.row_separator.unwrap_or_default();
                    for _ in 0..n {
                        print!("{}", separator);
                    }
                }
                Some(Token::Padding(n)) => {
                    for _ in 0..n {
                        print!(" ");
                    }
                }
                None => break,
            }
        }
    }
}

pub trait IntoTable {
    fn into_table(self) -> Table;
}

impl<T: Into<Cell>> IntoTable for Vec<Vec<T>> {
    fn into_table(self) -> Table {
        let mut table = Table::default();
        for row in self {
            table.push_row(row.into_iter().map(|c| c.into()).collect());
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::{IntoCell, IntoTable};
    use crate::table::style::Style;
    use crate::table::Alignment;
    use colored::Color;

    #[test]
    fn renders_empty() {
        let table = Vec::<Vec<String>>::new().into_table();

        assert_eq!("".to_string(), table.render(false));

        let table = vec![Vec::<String>::new(), vec![], vec![], vec![]].into_table();

        assert_eq!("\n\n\n\n".to_string(), table.render(false));

        let table = vec![vec!["", ""], vec![], vec![], vec![]].into_table();

        assert_eq!(" \n \n \n \n".to_string(), table.render(false));

        let table = vec![vec!["", ""], vec![], vec![], vec![]]
            .into_table()
            .with_column_separator('|');

        assert_eq!("|\n|\n|\n|\n".to_string(), table.render(false))
    }

    #[test]
    fn renders_simple() {
        let table = vec![
            vec!["simple", "test", "testcaselong"],
            vec!["loooooonger", "test", "shorter"],
            vec!["shorterrow"],
        ]
        .into_table()
        .with_headers(vec!["first", "second", "third"])
        .with_column_separator('|');

        assert_eq!(
            r#"
   first   |second|   third
  simple   | test |testcaselong
loooooonger| test |  shorter
shorterrow |      |            
"#
            .to_string(),
            format!("\n{}", table.render(false)),
        )
    }

    #[test]
    fn renders_no_headers() {
        let table = vec![
            vec!["simple", "test", "with", "no", "headers"],
            vec![],
            vec!["or", "a", "separator"],
        ]
        .into_table();

        assert_eq!(
            r#"
simple test   with    no headers
                                
  or    a   separator           
"#
            .to_string(),
            format!("\n{}", table.render(false)),
        )
    }

    #[test]
    fn alignment() {
        let table = vec![
            vec![
                "left".cell().left(),
                "center".cell(),
                "right".cell().right(),
            ],
            vec!["          ".cell(), " center ".cell(), "          ".cell()],
            vec![
                "right".cell().right(),
                "center".cell(),
                "left".cell().left(),
            ],
        ]
        .into_table()
        .with_column_separator('|');

        assert_eq!(
            r#"
left      | center |     right
          | center |          
     right| center |left
"#,
            format!("\n{}", table.render(false)),
        )
    }

    #[test]
    fn renders_separators() {
        let table = vec![
            vec!["first", "row"],
            vec!["second"],
            vec![],
            vec!["fourth", "row"],
        ]
        .into_table()
        .with_row_separator('~')
        .with_column_separator('|');

        assert_eq!(
            r#"
~~~~~~~~~~~
first |row
~~~~~~~~~~~
second|   
~~~~~~~~~~~
      |   
~~~~~~~~~~~
fourth|row
~~~~~~~~~~~
"#
            .to_string(),
            format!("\n{}", table.render(false)),
        )
    }

    #[test]
    fn simple_cell() {
        let cell = "".cell();
        assert_eq!(cell.alignment, Alignment::Center);
        assert_eq!(cell.text, "");
        assert_eq!(cell.color, Color::BrightWhite);
        assert_eq!(cell.style, Style::default());
    }

    #[test]
    fn cell_alignment() {
        let cell = "some text".cell().left();
        assert_eq!(cell.text, "some text");
        assert_eq!(cell.alignment, Alignment::Left);
        let cell = cell.right();
        assert_eq!(cell.alignment, Alignment::Right);
        let cell = cell.center();
        assert_eq!(cell.alignment, Alignment::Center);
    }

    #[test]
    fn cell_style() {
        let cell = "some text".cell();

        assert!(!cell.style.is_bold());
        assert!(!cell.style.is_italic());
        assert!(!cell.style.is_underline());
        assert!(!cell.style.is_reversed());

        let cell = cell
            .color(Color::Blue)
            .bold()
            .italic()
            .reversed()
            .underline();

        assert_eq!(cell.color, Color::Blue);
        assert!(cell.style.is_bold());
        assert!(cell.style.is_italic());
        assert!(cell.style.is_underline());
        assert!(cell.style.is_reversed());
    }
}
