//! In-memory token accumulation. Do not write one SQLite row per token.

#[derive(Debug, Default, Clone)]
pub struct TokenAccumulator {
    tokens: Vec<String>,
}

impl TokenAccumulator {
    pub fn new() -> Self {
        Self { tokens: Vec::new() }
    }

    pub fn push(&mut self, token: impl Into<String>) {
        self.tokens.push(token.into());
    }

    pub fn extend_from(&mut self, tokens: impl IntoIterator<Item = impl Into<String>>) {
        self.tokens.extend(tokens.into_iter().map(Into::into));
    }

    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn finalize_text(&self) -> String {
        self.tokens.concat()
    }

    pub fn tokens(&self) -> &[String] {
        &self.tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenates_without_inventing() {
        let mut a = TokenAccumulator::new();
        a.push("hel");
        a.push("lo");
        assert_eq!(a.finalize_text(), "hello");
        assert_eq!(a.token_count(), 2);
    }
}
