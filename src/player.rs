use std::collections::HashMap;

use crate::cards::{CardDefinition, CardTerm, Deck};
use crate::language::{
    EvaluationError, Expr, TypeInferenceError, TypeScheme, Value, evaluate, infer_monomorphic,
    is_valid_variable_name,
};

pub const STARTING_ENERGY: u32 = 3;

#[derive(Debug)]
pub struct Player {
    current_deck: Deck,
}

impl Player {
    pub fn new(deck: Deck) -> Self {
        Self { current_deck: deck }
    }
    pub const fn deck(&self) -> &Deck {
        &self.current_deck
    }
    pub const fn deck_mut(&mut self) -> &mut Deck {
        &mut self.current_deck
    }
}

/// Expression-backed cards and energy owned by one turn. Drop on completion or quit.
#[derive(Debug)]
pub struct TurnState {
    current_energy: u32,
    bindings: HashMap<String, CardDefinition>,
}

impl Default for TurnState {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnState {
    pub fn new() -> Self {
        Self {
            current_energy: STARTING_ENERGY,
            bindings: HashMap::new(),
        }
    }
    pub const fn current_energy(&self) -> u32 {
        self.current_energy
    }
    pub fn bindings(&self) -> &HashMap<String, CardDefinition> {
        &self.bindings
    }
    pub(crate) fn take_binding(&mut self, name: &str) -> Option<CardDefinition> {
        self.bindings.remove(name)
    }

    pub(crate) fn restore_binding(&mut self, card: CardDefinition) {
        // Clearing an older selection must not overwrite a newer binding.
        self.bindings.entry(card.name().to_owned()).or_insert(card);
    }
    pub fn sorted_bindings(&self) -> Vec<&CardDefinition> {
        let mut cards: Vec<_> = self.bindings.values().collect();
        cards.sort_by(|left, right| left.name().cmp(right.name()));
        cards
    }

    /// Type-check without evaluating. Rebinding replaces metadata atomically.
    pub fn bind_expression(&mut self, name: &str, expression: Expr) -> Result<(), PlayerError> {
        self.require_energy()?;
        if !is_valid_variable_name(name) {
            return Err(PlayerError::InvalidVariableName(name.to_owned()));
        }
        let typ = infer_monomorphic(&expression).map_err(PlayerError::TypeInference)?;
        self.bindings.insert(
            name.to_owned(),
            CardDefinition::new(
                name.to_owned(),
                CardTerm::Binding(expression),
                TypeScheme::monomorphic(typ),
            ),
        );
        self.current_energy -= 1;
        Ok(())
    }

    /// On success the coordinator ends the turn and drops this state.
    pub fn evaluate(&mut self, expression: &Expr) -> Result<i64, PlayerError> {
        self.require_energy()?;
        infer_monomorphic(expression).map_err(PlayerError::TypeInference)?;
        let value = evaluate(expression).map_err(PlayerError::Evaluation)?;
        let Value::Int(score) = value else {
            return Err(PlayerError::ScoreMustBeInteger);
        };
        self.current_energy -= 1;
        Ok(score)
    }

    fn require_energy(&self) -> Result<(), PlayerError> {
        if self.current_energy == 0 {
            Err(PlayerError::OutOfEnergy)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerError {
    OutOfEnergy,
    InvalidVariableName(String),
    TypeInference(TypeInferenceError),
    Evaluation(EvaluationError),
    ScoreMustBeInteger,
}

impl std::fmt::Display for PlayerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfEnergy => formatter.write_str("the player is out of energy"),
            Self::InvalidVariableName(name) => {
                write!(formatter, "{name:?} is not a valid variable name")
            }
            Self::ScoreMustBeInteger => formatter.write_str("only an Int can be used as a score"),
            Self::TypeInference(error) => write!(formatter, "type inference failed: {error:?}"),
            Self::Evaluation(error) => write!(formatter, "evaluation failed: {error:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Operator;

    #[test]
    fn bindings_preserve_expressions_including_functions_without_evaluation() {
        let mut turn = TurnState::new();
        let expressions = [
            Expr::Literal(Value::Bool(true)),
            Expr::Operator(Operator::Add),
            Expr::Apply(
                Box::new(Expr::Operator(Operator::Add)),
                Box::new(Expr::Literal(Value::Int(1))),
            ),
        ];
        for (index, expression) in expressions.into_iter().enumerate() {
            let name = format!("x{index}");
            let typ = infer_monomorphic(&expression).unwrap();
            turn.bind_expression(&name, expression.clone()).unwrap();
            let card = &turn.bindings()[&name];
            assert_eq!(card.term(), &CardTerm::Binding(expression));
            assert_eq!(card.scheme().typ, typ);
        }
        assert_eq!(turn.current_energy(), 0);
    }

    #[test]
    fn rebinding_replaces_expression_and_type() {
        let mut turn = TurnState::new();
        turn.bind_expression("x", Expr::Literal(Value::Int(1)))
            .unwrap();
        turn.bind_expression("x", Expr::Operator(Operator::Add))
            .unwrap();
        assert_eq!(turn.bindings().len(), 1);
        assert_eq!(turn.bindings()["x"].scheme().typ, Operator::Add.typ());
        assert_eq!(turn.current_energy(), 1);
    }

    #[test]
    fn failed_binding_preserves_previous_binding_and_energy() {
        let mut turn = TurnState::new();
        turn.bind_expression("x", Expr::Literal(Value::Int(5)))
            .unwrap();
        let before = turn.bindings().clone();
        assert!(
            turn.bind_expression("bad name", Expr::Literal(Value::Int(2)))
                .is_err()
        );
        assert!(
            turn.bind_expression("x", Expr::Variable("missing".into()))
                .is_err()
        );
        assert_eq!(turn.bindings(), &before);
        assert_eq!(turn.current_energy(), 2);
        turn.bind_expression("x", Expr::Literal(Value::Int(6)))
            .unwrap();
        turn.bind_expression("x", Expr::Literal(Value::Int(7)))
            .unwrap();
        assert_eq!(
            turn.bind_expression("x", Expr::Literal(Value::Int(8))),
            Err(PlayerError::OutOfEnergy)
        );
        assert_eq!(
            turn.bindings()["x"].term(),
            &CardTerm::Binding(Expr::Literal(Value::Int(7)))
        );
    }

    #[test]
    fn scoring_failures_preserve_turn_and_fresh_turn_has_no_bindings() {
        let mut turn = TurnState::new();
        turn.bind_expression("x", Expr::Literal(Value::Int(5)))
            .unwrap();
        assert_eq!(
            turn.evaluate(&Expr::Literal(Value::Bool(true))),
            Err(PlayerError::ScoreMustBeInteger)
        );
        assert!(turn.evaluate(&Expr::Operator(Operator::Add)).is_err());
        assert_eq!(turn.current_energy(), 2);
        assert_eq!(turn.bindings().len(), 1);
        assert_eq!(turn.evaluate(&Expr::Literal(Value::Int(5))), Ok(5));
        drop(turn);
        let mut next = TurnState::new();
        assert!(next.bindings().is_empty());
        assert_eq!(next.current_energy(), STARTING_ENERGY);
        next.bind_expression("x", Expr::Literal(Value::Int(2)))
            .unwrap();
    }
}
