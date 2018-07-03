use std::{
    mem,
    collections::VecDeque
};
use syntax::{
    Lexer,
    Result,
    SyntaxError,
    tree::*,
    token::*,
};

macro_rules! matches {
    ($pat:pat, $expr:expr) => {{ if let $pat = $expr { true } else { false } }}
}

pub struct Parser<'n, S>
    where S: Iterator<Item=char>
{
    lexer: Lexer<'n, S>,
    curr: Option<RangeToken<'n>>,
    next: Option<RangeToken<'n>>,
    stmt_level: usize,
}

impl<'n, S> Parser<'n, S>
    where S: Iterator<Item=char>
{
    pub fn new(input: S, source_name: &'n str) -> Self {
        let lexer = Lexer::new(input, source_name);
        Parser::from_lexer(lexer)
    }

    pub fn from_lexer(lexer: Lexer<'n, S>) -> Self {
        Parser {
            lexer,
            curr: None,
            next: None,
            stmt_level: 0,
        }
    }

    pub fn into_parse_tree(mut self) -> Result<'n, SyntaxTree<'n>> {
        self.init()?;
        if self.curr.is_some() {
            self.next_tree()
        } else {
            Ok(SyntaxTree::default())
        }
    }

    /// Readies this parser by filling in the first two tokens.
    fn init(&mut self) -> Result<'n, ()> {
        assert!(self.curr.is_none());
        assert!(self.next.is_none());
        // Option<Result<Token>> -> Option<Token>
        self.curr = if let Some(result) = self.lexer.next() {
            Some(result?)
        } else {
            None
        };

        self.next = if let Some(result) = self.lexer.next() {
            Some(result?)
        } else {
            None
        };
        Ok(())
    }

    fn next_tree(&mut self) -> Result<'n, SyntaxTree<'n>> {
        let mut stmts = vec![];
        while self.curr.is_some() {
            stmts.push(self.next_stmt()?);
        }
        Ok(SyntaxTree { stmts })
    }

    fn next_stmt(&mut self) -> Result<'n, Stmt<'n>> {
        assert_eq!(self.stmt_level, 0);
        while self.is_token_match(&Token::LineEnd) || self.is_token_match(&Token::NewLine) {
            self.next_token()?;
        }

        let curr = if let Some(curr) = self.curr.clone() {
            Token::from(curr)
        } else {
            return Err(self.err_expected_got_eof(Stmt::name()));
        };
        // TODO : expect EOL or EOF after every expression
        let stmt = match curr {
            Token::ContinueKw => {
                self.next_token_or_newline()?;
                Stmt::Continue
            }
            Token::BreakKw => {
                self.next_token_or_newline()?;
                Stmt::Continue
            }
            Token::WhileKw => {
                self.next_token()?;
                let condblock = self.next_condition_block()?;
                Stmt::While(condblock)
            }
            Token::LoopKw => {
                self.next_token()?;
                let block = self.next_block()?;
                Stmt::Loop(block)
            }
            Token::IfKw => {
                self.next_token()?;
                let if_block = self.next_condition_block()?;
                let mut else_block = None;
                let mut elseif_blocks = vec![];
                while self.is_token_match(&Token::ElseKw) {
                    self.next_token()?;
                    if self.is_token_match(&Token::IfKw) {
                        // else-if block
                        self.next_token()?;
                        elseif_blocks.push(self.next_condition_block()?);
                    } else {
                        // else block
                        else_block = Some(self.next_block()?);
                        break;
                    }
                }
                Stmt::If {
                    if_block,
                    elseif_blocks,
                    else_block,
                }
            }
            ref t if t.is_lookahead::<Expr>() => {
                // expr, assignment
                let lhs = self.next_expr()?;
                if self.is_curr_assign_op() {
                    let op = self.next_assign_op()?;
                    let rhs = self.next_expr()?;
                    Stmt::Assign(lhs, op, rhs)
                } else {
                    Stmt::Expr(lhs)
                }
            }
            _ => return Err(self.err_expected_got("statement", self.curr.as_ref())),
        };
        self.next_eol_or_eof()?;
        Ok(stmt)
    }

    fn next_eol_or_eof(&mut self) -> Result<'n, ()> {
        if self.is_token_match(&Token::LineEnd) || self.is_token_match(&Token::NewLine) {
            self.next_token().map(|_| ())
        } else if self.curr.is_none() {
            Ok(())
        } else {
            Err(self.err_expected_got("end-of-line (newline or `;`) or EOF", self.curr.as_ref()))
        }
    }

    fn next_condition_block(&mut self) -> Result<'n, ConditionBlock<'n>> {
        let condition = self.next_expr()?;
        let block = self.next_block()?;
        Ok(ConditionBlock::new(condition, block))
    }

    fn next_block(&mut self) -> Result<'n, Block<'n>> {
        self.match_token(Token::LBrace)?;
        let mut stmts = vec![];
        while !self.is_token_match(&Token::RBrace) {
            let stmt = self.next_stmt()?;
            stmts.push(stmt);
        }
        self.match_token_preserve_newline(Token::RBrace)?;
        Ok(stmts)
    }

    fn next_expr(&mut self) -> Result<'n, Expr<'n>> {
        let op_queue = VecDeque::from(vec![
            vec![Op::DoublePercent, Op::DoubleEquals, Op::DoubleTilde, Op::NotEquals,
                 Op::LessEquals, Op::GreaterEquals, Op::Less, Op::Greater],
            vec![Op::Tilde],
            vec![Op::Plus, Op::Minus],
            vec![Op::Splat, Op::FSlash], ]);
        self.next_binary_expr(op_queue)
    }

    fn next_binary_expr(&mut self, mut op_queue: VecDeque<Vec<Op>>) -> Result<'n, Expr<'n>> {
        if let Some(top) = op_queue.pop_front() {
            let lhs = self.next_binary_expr(op_queue.clone())?;
            let op_matches = self.curr.as_ref()
                .map(|t| if let &Token::Op(ref op) = t.token() {
                    top.contains(op)
                } else {
                    false
                })
                .unwrap_or(false);
            if op_matches {
                let op = self.next_op()?;
                op_queue.push_front(top);
                let rhs = self.next_binary_expr(op_queue)?;
                Ok(Expr::Binary(Box::new(lhs), op, Box::new(rhs)))
            } else {
                Ok(lhs)
            }
        } else {
            self.next_unary_expr()
        }
    }


    fn next_unary_expr(&mut self) -> Result<'n, Expr<'n>> {
        if self.is_curr_op() {
            let token = self.next_token()?.unwrap();
            if token.is_lookahead::<Expr>() {
                let op = Token::from(token).into_op();
                let expr = self.next_unary_expr()?;
                Ok(Expr::Unary(op, Box::new(expr)))
            } else {
                Err(self.err_expected_got("unary operator", Some(&token)))
            }
        } else {
            self.next_atom_expr()
        }
    }

    fn next_atom_expr(&mut self) -> Result<'n, Expr<'n>> {
        let curr = if let Some(curr) = self.curr.clone() {
            Token::from(curr)
        } else {
            return Err(self.err_expected_got_eof(Expr::name()));
        };

        if !curr.is_lookahead::<Expr>() {
            return Err(self.err_expected_got(Expr::name(), self.curr.as_ref()));
        }

        let mut expr = match curr {
            Token::LParen => {
                self.next_token()?;
                self.stmt_level += 1;
                let inner = self.next_expr()?;
                self.stmt_level -= 1;
                // stmt_level is set to 0 at the start of each stmt rule, so stmts that end in
                // expressions are *required* to have a newline at the end
                if self.stmt_level == 0 {
                    self.match_token_preserve_newline(Token::RParen)?;
                } else {
                    self.match_token(Token::RParen)?;
                }
                inner
            }
            _ => if self.stmt_level == 0 {
                Expr::Atom(self.next_token_or_newline()?.unwrap())
            } else {
                Expr::Atom(self.next_token()?.unwrap())
            }

        };

        if self.is_token_match(&Token::LParen) {
            let args = self.next_funcall_args()?;
            expr = Expr::FunCall { function: Box::new(expr), args }
        }

        if self.is_token_match(&Token::LBracket) {
            self.next_token()?;
            let index = self.next_expr()?;
            self.match_token(Token::RBracket)?;
            Ok(Expr::ArrayAccess{ array: Box::new(expr), index: Box::new(index) })
        } else {
            Ok(expr)
        }
    }

    fn next_funcall_args(&mut self) -> Result<'n, Vec<Expr<'n>>> {
        self.match_token(Token::LParen)?;
        let mut args = vec![];
        if !self.is_token_match(&Token::RParen) {
            args.push(self.next_expr()?);
            while self.is_token_match(&Token::Comma) {
                self.next_token()?;
                args.push(self.next_expr()?);
            }
        }
        if self.stmt_level == 0 {
            self.match_token_preserve_newline(Token::RParen)?;
        } else {
            self.match_token(Token::RParen)?;
        }
        Ok(args)
    }

    fn next_op(&mut self) -> Result<'n, Op> {
        let matches = if let Some(&Token::Op(_)) = self.curr.as_ref().map(|r| r.token()) { true }
                      else { false };
        if matches {
            Ok(Token::from(self.next_token()?.unwrap()).into_op())
        } else {
            Err(self.err_expected_got("operator", self.curr.as_ref()))
        }
    }

    fn next_assign_op(&mut self) -> Result<'n, AssignOp> {
        let matches = matches!(Some(&Token::AssignOp(_)), self.curr.as_ref().map(|r| r.token()));
        if matches {
            Ok(Token::from(self.next_token()?.unwrap()).into_assign_op())
        } else {
            Err(self.err_expected_got("operator", self.curr.as_ref()))
        }
    }

    fn is_curr_op(&self) -> bool {
        self.curr.as_ref()
            .map(|t| matches!(&Token::Op(_), t.token()))
            .unwrap_or(false)
    }

    fn is_curr_assign_op(&self) -> bool {
        self.curr.as_ref()
            .map(|t| matches!(&Token::AssignOp(_), t.token()))
            .unwrap_or(false)
    }

    fn is_token_match(&mut self, token: &Token) -> bool {
        if let Some(ref curr) = self.curr {
            curr.token() == token
        } else {
            false
        }
    }

    fn match_token_preserve_newline(&mut self, token: Token) -> Result<'n, RangeToken<'n>> {
        if self.curr.as_ref().map(|r| r.token() == &token).unwrap_or(false) {
            self.next_token_or_newline()?
                .ok_or_else(|| self.err_expected_got_eof(token.to_string()))
        } else {
            let expected = token.to_string();
            Err(self.err_expected_got(expected, self.curr.as_ref()))
        }
    }

    fn match_token(&mut self, token: Token) -> Result<'n, RangeToken<'n>> {
        if self.curr.as_ref().map(|r| r.token() == &token).unwrap_or(false) {
            self.next_token()?
                .ok_or_else(|| self.err_expected_got_eof(token.to_string()))
        } else {
            let expected = token.to_string();
            Err(self.err_expected_got(expected, self.curr.as_ref()))
        }
    }

    /// Advances the lexer by one token, skipping over any comments as necessary.
    ///
    /// This method will not skip over newlines, and will instead return them as part of the normal
    /// token stream.
    fn next_token_or_newline(&mut self) -> Result<'n, Option<RangeToken<'n>>> {
        let mut next = self.lexer.next();
        while let Some(token) = next {
            let range_token = token?;
            if range_token.token() != &Token::Comment {
                next = Some(Ok(range_token));
                break;
            }
            next = self.lexer.next();
        }

        let next = if let Some(result) = next {
            Some(result?)
        } else {
            None
        };
        Ok(mem::replace(&mut self.curr, mem::replace(&mut self.next, next)))
    }

    /// Advances the lexer by one token, skipping over any comments and newlines as necessary.
    ///
    /// This skips over newlines, since, *for the most part*, the language is newline-agnostic.
    /// Only statements are required to be ended with either newlines *or* line-end characters.
    fn next_token(&mut self) -> Result<'n, Option<RangeToken<'n>>> {
        let mut token = self.next_token_or_newline()?;
        while self.is_token_match(&Token::NewLine) {
            token = self.next_token_or_newline()?;
        }
        Ok(token)
    }

    /// Creates a new `SyntaxError` using the supplied expected item.
    /// # Args
    /// `expected` - the expected item.
    ///
    /// # Returns
    /// A `SyntaxError` with a synthesized error message based on the `expected` param, with the
    /// `got` message being an EOF.
    fn err_expected_got_eof(&self, expected: impl AsRef<str>) -> SyntaxError<'n> {
        let message = format!("expected {}, but got EOF instead", expected.as_ref());
        self.err(message)
    }

    /// Creates a new `SyntaxError` using the supplied expected item and the actual item
    /// encountered.
    ///
    /// # Args
    /// `expected` - the expected item.
    /// `got` - the item that was encountered in the parser.
    ///
    /// # Returns
    /// A `SyntaxError` with a synthesized error message based on the `expected` and `got` params.
    fn err_expected_got<T: ToString>(&self, expected: impl AsRef<str>, got: Option<&T>) -> SyntaxError<'n> {
        let message = format!("expected {}, but got {} instead",
                              expected.as_ref(),
                              got.map(|s| s.to_string()).unwrap_or("EOF".to_string()));
        self.err(message)
    }

    /// Creates a new `SyntaxError` using the supplied message.
    ///
    /// # Args
    /// `message` - detailed error info.
    ///
    /// # Returns
    /// A `SyntaxError` with this parser's current position, as well the specified message.
    fn err(&self, message: String) -> SyntaxError<'n> {
        SyntaxError::new(message, self.lexer.pos())
    }
}

#[cfg(test)]
mod test {
    use syntax::*;
    use syntax::token::*;
    use syntax::tree::*;

    macro_rules! test_parser {
        ($input:expr) => {{
            let mut parser = Parser::new($input.chars(), "test");
            parser.init().unwrap();
            parser
        }};
    }

    macro_rules! token {
        ($($token:tt)+) => { RangeToken::new(Pos::default(), Pos::default(), $($token)+) }
    }

    #[test]
    fn test_parser_expr() {
        let mut parser = test_parser!("(1 + 2)");
        let expr = parser.next_expr().unwrap();
        assert_eq!(expr,
                   Expr::Binary(
                       Box::new(Expr::Atom(token!(Token::IntLit("1".to_string(), 10)))),
                       Op::Plus,
                       Box::new(Expr::Atom(token!(Token::IntLit("2".to_string(), 10))))
                       )
                  );
    }

    #[test]
    fn test_parser_match_token() {
        let mut parser = test_parser!("(1 + 2)");
        parser.match_token(Token::LParen).unwrap();
        parser.match_token(Token::IntLit("1".to_string(), 10)).unwrap();
        parser.match_token(Token::Op(Op::Plus)).unwrap();
        parser.match_token(Token::IntLit("2".to_string(), 10)).unwrap();
        parser.match_token(Token::RParen).unwrap();
    }
}

