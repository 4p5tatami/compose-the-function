use std::fmt::{self, Display};

use crate::cards::{CardDefinition, CardId, CardInstance, CardLibrary, CardTerm, Deck};
use crate::language::{Expr, Operator};
use crate::player::TurnState;

#[derive(Debug, Default)]
pub struct ExpressionBuilder {
    current_expr: String,
    selected_cards: Vec<SelectedCard>,
}

impl ExpressionBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_expr(&self) -> &str {
        &self.current_expr
    }

    pub fn select(
        &mut self,
        deck_index: usize,
        deck: &mut Deck,
        library: &CardLibrary,
    ) -> Result<(), ExpressionBuildError> {
        let instance = *deck
            .cards()
            .get(deck_index)
            .ok_or(ExpressionBuildError::InvalidDeckIndex(deck_index))?;
        let definition = library
            .get(instance.definition())
            .ok_or(ExpressionBuildError::UnknownCard(instance.definition()))?;

        let part = match definition.term() {
            CardTerm::Literal(value) => ExpressionPart::Operand(Expr::Literal(*value)),
            CardTerm::Binding(expr) | CardTerm::Definition(expr) => {
                ExpressionPart::Operand(expr.clone())
            }
            CardTerm::Operator(operator) => ExpressionPart::Operator(*operator),
        };
        let name = definition.name().to_owned();

        let removed = deck
            .remove(deck_index)
            .expect("the selected deck index was checked above");
        debug_assert_eq!(removed, instance);

        if !self.current_expr.is_empty() {
            self.current_expr.push(' ');
        }
        self.current_expr.push_str(&name);
        self.selected_cards.push(SelectedCard {
            source: SelectedSource::Deck(instance),
            part,
        });

        Ok(())
    }

    /// Reserve this binding until the selection is committed or cleared.
    pub fn select_binding(
        &mut self,
        name: &str,
        turn: &mut TurnState,
    ) -> Result<(), ExpressionBuildError> {
        let card = turn
            .take_binding(name)
            .ok_or_else(|| ExpressionBuildError::UnknownBinding(name.to_owned()))?;
        let CardTerm::Binding(expression) = card.term() else {
            unreachable!("turn registration only creates binding cards");
        };
        if !self.current_expr.is_empty() {
            self.current_expr.push(' ');
        }
        self.current_expr.push_str(name);
        let part = ExpressionPart::Operand(expression.clone());
        self.selected_cards.push(SelectedCard {
            source: SelectedSource::Binding(card),
            part,
        });
        Ok(())
    }

    pub fn has_selected_binding(&self, name: &str) -> bool {
        self.selected_cards.iter().any(|selected| {
            matches!(&selected.source, SelectedSource::Binding(card) if card.name() == name)
        })
    }

    /// Consume both deck cards and bindings after a successful action.
    pub fn commit(&mut self) {
        self.selected_cards.clear();
        self.current_expr.clear();
    }

    pub fn clear(&mut self, deck: &mut Deck, turn: &mut TurnState) {
        for selected in self.selected_cards.drain(..) {
            match selected.source {
                SelectedSource::Deck(instance) => deck.add(instance),
                SelectedSource::Binding(card) => turn.restore_binding(card),
            }
        }
        self.current_expr.clear();
    }

    pub fn build(&self) -> Result<Expr, ExpressionBuildError> {
        if self.selected_cards.is_empty() {
            return Err(ExpressionBuildError::EmptyExpression);
        }

        let mut operands = Vec::new();
        let mut operators = Vec::new();
        let mut expects_operand = true;

        for (index, selected) in self.selected_cards.iter().enumerate() {
            match (&selected.part, expects_operand) {
                (ExpressionPart::Operand(expression), true) => {
                    operands.push(expression.clone());
                    expects_operand = false;
                }
                (ExpressionPart::Operator(operator), false) => {
                    while operators.last().is_some_and(|previous: &Operator| {
                        previous.precedence() >= operator.precedence()
                    }) {
                        reduce_once(&mut operands, &mut operators)?;
                    }
                    operators.push(*operator);
                    expects_operand = true;
                }
                (ExpressionPart::Operator(_), true) => {
                    return Err(ExpressionBuildError::ExpectedOperand(index + 1));
                }
                (ExpressionPart::Operand(_), false) => {
                    return Err(ExpressionBuildError::ExpectedOperator(index + 1));
                }
            }
        }

        if expects_operand {
            return Err(ExpressionBuildError::TrailingOperator);
        }

        while !operators.is_empty() {
            reduce_once(&mut operands, &mut operators)?;
        }

        operands.pop().ok_or(ExpressionBuildError::EmptyExpression)
    }
}

fn reduce_once(
    operands: &mut Vec<Expr>,
    operators: &mut Vec<Operator>,
) -> Result<(), ExpressionBuildError> {
    let operator = operators
        .pop()
        .ok_or(ExpressionBuildError::MalformedExpression)?;
    let right = operands
        .pop()
        .ok_or(ExpressionBuildError::MalformedExpression)?;
    let left = operands
        .pop()
        .ok_or(ExpressionBuildError::MalformedExpression)?;

    operands.push(Expr::Apply(
        Box::new(Expr::Apply(
            Box::new(Expr::Operator(operator)),
            Box::new(left),
        )),
        Box::new(right),
    ));
    Ok(())
}

#[derive(Debug)]
struct SelectedCard {
    source: SelectedSource,
    part: ExpressionPart,
}

#[derive(Debug)]
enum SelectedSource {
    Deck(CardInstance),
    Binding(CardDefinition),
}

#[derive(Debug)]
enum ExpressionPart {
    Operand(Expr),
    Operator(Operator),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpressionBuildError {
    EmptyExpression,
    InvalidDeckIndex(usize),
    UnknownCard(CardId),
    UnknownBinding(String),
    ExpectedOperand(usize),
    ExpectedOperator(usize),
    TrailingOperator,
    MalformedExpression,
}

impl Display for ExpressionBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyExpression => formatter.write_str("expression is empty"),
            Self::InvalidDeckIndex(_) => formatter.write_str("selected card is not in the deck"),
            Self::UnknownCard(id) => write!(formatter, "card {} is missing", id.value()),
            Self::UnknownBinding(name) => {
                write!(formatter, "binding {name:?} is missing from this turn")
            }
            Self::ExpectedOperand(position) => {
                write!(formatter, "expected a value at card {position}")
            }
            Self::ExpectedOperator(position) => {
                write!(formatter, "expected an operator at card {position}")
            }
            Self::TrailingOperator => formatter.write_str("expression cannot end with an operator"),
            Self::MalformedExpression => formatter.write_str("expression is malformed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::starter_cards;
    use crate::language::{Value, evaluate, infer_monomorphic};

    #[test]
    fn clearing_an_old_selection_preserves_a_newer_binding_with_the_same_name() {
        let (_, mut deck) = starter_cards();
        let mut turn = TurnState::new();
        turn.bind_expression("x", Expr::Literal(Value::Int(1)))
            .unwrap();
        let mut builder = ExpressionBuilder::new();
        builder.select_binding("x", &mut turn).unwrap();
        turn.bind_expression("x", Expr::Literal(Value::Int(9)))
            .unwrap();
        builder.clear(&mut deck, &mut turn);
        assert_eq!(turn.bindings().len(), 1);
        assert_eq!(
            turn.bindings()["x"].term(),
            &CardTerm::Binding(Expr::Literal(Value::Int(9)))
        );
    }

    #[test]
    fn a_binding_can_only_be_selected_once_and_clear_restores_it() {
        let (library, mut deck) = starter_cards();
        let mut turn = TurnState::new();
        turn.bind_expression("x", Expr::Literal(Value::Int(3)))
            .unwrap();
        let mut builder = ExpressionBuilder::new();
        builder.select_binding("x", &mut turn).unwrap();
        builder.select(3, &mut deck, &library).unwrap();
        assert_eq!(
            builder.select_binding("x", &mut turn),
            Err(ExpressionBuildError::UnknownBinding("x".into()))
        );
        assert_eq!(builder.current_expr(), "x +");
        assert!(turn.bindings().is_empty());
        builder.clear(&mut deck, &mut turn);
        assert_eq!(deck.cards().len(), 4);
        assert_eq!(turn.bindings().len(), 1);
        builder.clear(&mut deck, &mut turn);
        assert_eq!(deck.cards().len(), 4);
    }

    #[test]
    fn rebinding_preserves_old_selections_and_derived_bindings() {
        let mut turn = TurnState::new();
        let mut builder = ExpressionBuilder::new();
        turn.bind_expression("x", Expr::Literal(Value::Int(1)))
            .unwrap();
        builder.select_binding("x", &mut turn).unwrap();
        turn.bind_expression("y", builder.build().unwrap()).unwrap();
        turn.bind_expression("x", Expr::Literal(Value::Int(9)))
            .unwrap();
        assert_eq!(evaluate(&builder.build().unwrap()), Ok(Value::Int(1)));
        builder.commit();
        builder.select_binding("y", &mut turn).unwrap();
        assert_eq!(evaluate(&builder.build().unwrap()), Ok(Value::Int(1)));
        builder.commit();
        builder.select_binding("x", &mut turn).unwrap();
        assert_eq!(evaluate(&builder.build().unwrap()), Ok(Value::Int(9)));
    }

    #[test]
    fn a_missing_binding_does_not_change_the_selection() {
        let mut builder = ExpressionBuilder::new();
        assert_eq!(
            builder.select_binding("x", &mut TurnState::new()),
            Err(ExpressionBuildError::UnknownBinding("x".into()))
        );
        assert_eq!(builder.current_expr(), "");
        assert_eq!(builder.build(), Err(ExpressionBuildError::EmptyExpression));
    }

    #[test]
    fn selected_cards_build_an_infix_expression_and_leave_the_deck() {
        let (library, mut deck) = starter_cards();
        let mut builder = ExpressionBuilder::new();

        builder.select(0, &mut deck, &library).unwrap(); // 1
        builder.select(2, &mut deck, &library).unwrap(); // +
        builder.select(0, &mut deck, &library).unwrap(); // 2

        let expression = builder.build().unwrap();
        assert_eq!(builder.current_expr(), "1 + 2");
        assert_eq!(infer_monomorphic(&expression).unwrap().to_string(), "Int");
        assert_eq!(evaluate(&expression), Ok(Value::Int(3)));
        assert_eq!(deck.cards().len(), 1);
    }

    #[test]
    fn invalid_order_is_rejected_without_clearing_the_selection() {
        let (library, mut deck) = starter_cards();
        let mut builder = ExpressionBuilder::new();

        builder.select(0, &mut deck, &library).unwrap(); // 1
        builder.select(0, &mut deck, &library).unwrap(); // 2

        assert_eq!(builder.current_expr(), "1 2");
        assert_eq!(
            builder.build(),
            Err(ExpressionBuildError::ExpectedOperator(2))
        );
    }

    #[test]
    fn clear_returns_selected_instances_to_the_deck() {
        let (library, mut deck) = starter_cards();
        let mut turn = TurnState::new();
        let mut builder = ExpressionBuilder::new();

        builder.select(0, &mut deck, &library).unwrap();
        builder.select(2, &mut deck, &library).unwrap();
        builder.clear(&mut deck, &mut turn);

        assert_eq!(builder.current_expr(), "");
        assert_eq!(deck.cards().len(), 4);
    }

    #[test]
    fn multiplication_has_higher_precedence_than_addition() {
        let mut library = CardLibrary::new();
        let one = library.register_integer_literal(1);
        let add = library.register_operator(Operator::Add);
        let two = library.register_integer_literal(2);
        let multiply = library.register_operator(Operator::Multiply);
        let five = library.register_integer_literal(5);
        let mut deck = Deck::new(
            [one, add, two, multiply, five]
                .map(CardInstance::new)
                .to_vec(),
        );
        let mut builder = ExpressionBuilder::new();

        for _ in 0..5 {
            builder.select(0, &mut deck, &library).unwrap();
        }

        let expression = builder.build().unwrap();
        assert_eq!(builder.current_expr(), "1 + 2 * 5");
        assert_eq!(evaluate(&expression), Ok(Value::Int(11)));
    }
}
