use std::collections::HashMap;

use crate::language::{Expr, Operator, Type, TypeScheme, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CardId(u32);

impl CardId {
    pub const fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardTerm {
    Literal(Value),
    Operator(Operator),
    Binding(Expr),
    Definition(Expr),
}

impl CardTerm {
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Literal(_) => "Literal",
            Self::Operator(_) => "Operator",
            Self::Binding(_) => "Binding",
            Self::Definition(_) => "Definition",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardDefinition {
    name: String,
    term: CardTerm,
    scheme: TypeScheme,
}

impl CardDefinition {
    pub(crate) fn new(name: String, term: CardTerm, scheme: TypeScheme) -> Self {
        Self { name, term, scheme }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn term(&self) -> &CardTerm {
        &self.term
    }

    pub const fn scheme(&self) -> &TypeScheme {
        &self.scheme
    }
}

#[derive(Debug, Default)]
pub struct CardLibrary {
    definitions: HashMap<CardId, CardDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardRegistrationError {
    BindingMustBeTurnLocal,
}

impl CardLibrary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        name: impl Into<String>,
        term: CardTerm,
        scheme: TypeScheme,
    ) -> Result<CardId, CardRegistrationError> {
        if matches!(term, CardTerm::Binding(_)) {
            return Err(CardRegistrationError::BindingMustBeTurnLocal);
        }
        let id = CardId(self.definitions.len() as u32 + 1);

        self.definitions
            .insert(id, CardDefinition::new(name.into(), term, scheme));
        Ok(id)
    }

    pub fn register_integer_literal(&mut self, value: i64) -> CardId {
        self.register(
            value.to_string(),
            CardTerm::Literal(Value::Int(value)),
            TypeScheme::monomorphic(Type::Int),
        )
        .expect("literal cards are persistent")
    }

    pub fn register_operator(&mut self, operator: Operator) -> CardId {
        self.register(
            operator.symbol(),
            CardTerm::Operator(operator),
            TypeScheme::monomorphic(operator.typ()),
        )
        .expect("operator cards are persistent")
    }

    pub fn get(&self, id: CardId) -> Option<&CardDefinition> {
        self.definitions.get(&id)
    }

    pub fn contains_name(&self, name: &str) -> bool {
        self.definitions
            .values()
            .any(|definition| definition.name() == name)
    }

    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardInstance {
    definition: CardId,
}

impl CardInstance {
    pub const fn new(definition: CardId) -> Self {
        Self { definition }
    }

    pub const fn definition(&self) -> CardId {
        self.definition
    }
}

#[derive(Debug, Default)]
pub struct Deck {
    cards: Vec<CardInstance>,
}

impl Deck {
    pub fn new(cards: Vec<CardInstance>) -> Self {
        Self { cards }
    }

    pub fn add(&mut self, card: CardInstance) {
        self.cards.push(card);
    }

    pub fn remove(&mut self, index: usize) -> Option<CardInstance> {
        if index < self.cards.len() {
            Some(self.cards.remove(index))
        } else {
            None
        }
    }

    pub fn contains(&self, id: CardId) -> bool {
        self.cards.iter().any(|card| card.definition() == id)
    }

    pub fn cards(&self) -> &[CardInstance] {
        &self.cards
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }
}

pub fn starter_cards() -> (CardLibrary, Deck) {
    let mut library = CardLibrary::new();
    let mut instances = [1, 2, 5]
        .map(|value| CardInstance::new(library.register_integer_literal(value)))
        .to_vec();
    instances.push(CardInstance::new(library.register_operator(Operator::Add)));

    (library, Deck::new(instances))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_library_rejects_bindings_without_consuming_an_id() {
        let mut library = CardLibrary::new();
        let first = library.register_integer_literal(1);
        assert_eq!(
            library.register(
                "x",
                CardTerm::Binding(Expr::Literal(Value::Int(2))),
                TypeScheme::monomorphic(Type::Int)
            ),
            Err(CardRegistrationError::BindingMustBeTurnLocal)
        );
        assert_eq!(library.len(), 1);
        assert_eq!(
            library.register_integer_literal(2).value(),
            first.value() + 1
        );
    }

    #[test]
    fn library_generates_incrementing_card_ids() {
        let mut library = CardLibrary::new();

        let first = library.register_integer_literal(1);
        let second = library.register_integer_literal(2);

        assert_eq!(first.value(), 1);
        assert_eq!(second.value(), 2);
    }

    #[test]
    fn starter_deck_references_literal_definitions() {
        let (library, deck) = starter_cards();

        let values: Vec<_> = deck
            .cards()
            .iter()
            .filter_map(|instance| {
                let definition = library.get(instance.definition()).unwrap();
                match definition.term() {
                    CardTerm::Literal(Value::Int(value)) => Some(*value),
                    _ => None,
                }
            })
            .collect();

        assert_eq!(values, vec![1, 2, 5]);
        assert!(deck.cards().iter().any(|instance| {
            library
                .get(instance.definition())
                .is_some_and(|definition| definition.term() == &CardTerm::Operator(Operator::Add))
        }));
    }

    #[test]
    fn registering_an_operator_uses_its_semantics_for_metadata() {
        let mut library = CardLibrary::new();

        let add_id = library.register_operator(Operator::Add);
        let comparison_id = library.register_operator(Operator::GreaterThanOrEqual);

        let add = library.get(add_id).unwrap();
        assert_eq!(add.name(), "+");
        assert_eq!(add.term(), &CardTerm::Operator(Operator::Add));
        assert_eq!(add.scheme().typ.to_string(), "Int -> Int -> Int");

        let comparison = library.get(comparison_id).unwrap();
        assert_eq!(comparison.name(), ">=");
        assert_eq!(comparison.scheme().typ.to_string(), "Int -> Int -> Bool");
    }
}
