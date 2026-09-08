use std::io::{self, Write, stdout};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind},
    execute, queue,
    style::{Attribute, Print, SetAttribute},
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};

use crate::cards::{CardDefinition, CardLibrary, Deck};
use crate::expression_builder::ExpressionBuilder;
use crate::language::{Expr, infer_monomorphic, is_valid_variable_name};
use crate::player::TurnState;

const CARD_HEIGHT: usize = 5;
const MINIMUM_INNER_WIDTH: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildAction {
    Scored(i64),
    Quit,
}

/// Owns the turn until scoring, quitting, or an I/O failure discards its bindings.
pub fn construct_expression(library: &CardLibrary, deck: &mut Deck) -> io::Result<BuildAction> {
    let mut turn = TurnState::new();
    let mut builder = ExpressionBuilder::new();
    let result = construct_in_turn(library, deck, &mut turn, &mut builder);
    // Return uncommitted cards before discarding the turn and its bindings.
    builder.clear(deck, &mut turn);
    result
}

fn construct_in_turn(
    library: &CardLibrary,
    deck: &mut Deck,
    turn: &mut TurnState,
    builder: &mut ExpressionBuilder,
) -> io::Result<BuildAction> {
    let mut selected_index = 0;
    let mut status = String::new();
    let _terminal = TerminalGuard::enter()?;
    let mut output = stdout();

    loop {
        render(
            &mut output,
            library,
            deck,
            builder,
            selected_index,
            &status,
            turn,
        )?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        status.clear();
        let cards_count = deck.cards().len();
        let bindings_count = turn.bindings().len();
        let count = cards_count + bindings_count;
        let (group_start, group_count) = if selected_index < cards_count {
            (0, cards_count)
        } else {
            (cards_count, bindings_count)
        };
        match key.code {
            KeyCode::Up if selected_index >= cards_count && cards_count > 0 => {
                selected_index = 0;
            }
            KeyCode::Down if selected_index < cards_count && bindings_count > 0 => {
                selected_index = cards_count;
            }
            KeyCode::Left if group_count > 0 => {
                selected_index = if selected_index == group_start {
                    group_start + group_count - 1
                } else {
                    selected_index - 1
                };
            }
            KeyCode::Right if group_count > 0 => {
                selected_index = group_start + (selected_index - group_start + 1) % group_count;
            }
            KeyCode::Enter | KeyCode::Char(' ') if count > 0 => {
                let result = if selected_index < deck.cards().len() {
                    builder.select(selected_index, deck, library)
                } else {
                    let name = turn.sorted_bindings()[selected_index - cards_count]
                        .name()
                        .to_owned();
                    builder.select_binding(&name, turn)
                };
                if let Err(error) = result {
                    status = format!("Could not select card: {error}");
                }
                if selected_index < cards_count {
                    selected_index = selected_index.min(deck.cards().len().saturating_sub(1));
                } else if turn.bindings().is_empty() {
                    selected_index = 0;
                } else {
                    selected_index = selected_index.min(cards_count + turn.bindings().len() - 1);
                }
            }
            KeyCode::Char(character) if character.eq_ignore_ascii_case(&'b') => {
                if turn.current_energy() == 0 {
                    status = "Could not bind: the player is out of energy".into();
                    continue;
                }
                if let Err(error) = validated_expression(builder) {
                    status = error;
                    continue;
                }
                if let Some(name) =
                    read_variable_name(&mut output, library, deck, builder, selected_index, turn)?
                {
                    match bind_selection(builder, turn, &name) {
                        Ok(()) => {
                            status = format!("Bound {name}. One use this turn.");
                            selected_index = 0;
                        }
                        Err(error) => status = error,
                    }
                } else {
                    status = "Binding cancelled.".into();
                }
            }
            KeyCode::Char(character) if character.eq_ignore_ascii_case(&'e') => {
                match score_selection(builder, turn) {
                    Ok(score) => return Ok(BuildAction::Scored(score)),
                    Err(error) => status = error,
                }
            }
            KeyCode::Char(character) if character.eq_ignore_ascii_case(&'c') => {
                builder.clear(deck, turn);
                selected_index = 0;
                status = "Selection cleared.".into();
            }
            KeyCode::Esc | KeyCode::Char('q') => return Ok(BuildAction::Quit),
            KeyCode::Enter | KeyCode::Char(' ') => {
                status = "No cards remain. Bind, end the turn, or clear.".into();
            }
            _ => {}
        }
    }
}

fn bind_selection(
    builder: &mut ExpressionBuilder,
    turn: &mut TurnState,
    name: &str,
) -> Result<(), String> {
    let expression = validated_expression(builder)?;
    turn.bind_expression(name, expression)
        .map_err(|error| format!("Could not bind expression: {error}"))?;
    builder.commit();
    Ok(())
}

fn score_selection(builder: &mut ExpressionBuilder, turn: &mut TurnState) -> Result<i64, String> {
    let expression = validated_expression(builder)?;
    let score = turn
        .evaluate(&expression)
        .map_err(|error| format!("Could not end turn: {error}"))?;
    builder.commit();
    Ok(score)
}

fn overwrite_warning(turn: &TurnState, builder: &ExpressionBuilder, name: &str) -> Option<String> {
    (turn.bindings().contains_key(name) || builder.has_selected_binding(name)).then(|| {
        format!("Warning: Enter will overwrite binding {name:?}. Earlier expressions keep its previous definition.")
    })
}

fn read_variable_name(
    output: &mut impl Write,
    library: &CardLibrary,
    deck: &Deck,
    builder: &ExpressionBuilder,
    selected: usize,
    turn: &TurnState,
) -> io::Result<Option<String>> {
    let mut name = String::new();
    let mut status = String::new();
    loop {
        render(output, library, deck, builder, selected, &status, turn)?;
        queue!(
            output,
            Print("\r\n\r\nVariable name: "),
            Print(&name),
            SetAttribute(Attribute::Reverse),
            Print(" "),
            SetAttribute(Attribute::Reset),
            Print("\r\nEnter: confirm  Esc: cancel")
        )?;
        if let Some(warning) = overwrite_warning(turn, builder, &name) {
            queue!(output, Print("\r\n"), Print(warning))?;
        }
        output.flush()?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        status.clear();
        match key.code {
            KeyCode::Enter if !is_valid_variable_name(&name) => {
                status = "Variable names must start with a letter or underscore and contain only letters, numbers, or underscores.".into();
            }
            KeyCode::Enter => return Ok(Some(name)),
            KeyCode::Esc => return Ok(None),
            KeyCode::Backspace => {
                name.pop();
            }
            KeyCode::Char(character) if !character.is_control() => name.push(character),
            _ => {}
        }
    }
}

fn validated_expression(builder: &ExpressionBuilder) -> Result<Expr, String> {
    let expression = builder
        .build()
        .map_err(|error| format!("Invalid expression: {error}"))?;
    infer_monomorphic(&expression)
        .map_err(|error| format!("Invalid expression type: {error:?}"))?;
    Ok(expression)
}

fn render(
    output: &mut impl Write,
    library: &CardLibrary,
    deck: &Deck,
    builder: &ExpressionBuilder,
    selected: usize,
    status: &str,
    turn: &TurnState,
) -> io::Result<()> {
    let cards = resolve_cards(library, deck)?;
    let current_expr = if builder.current_expr().is_empty() {
        "<empty>"
    } else {
        builder.current_expr()
    };
    let energy = turn.current_energy();
    queue!(
        output,
        MoveTo(0, 0),
        Clear(ClearType::All),
        Print(format!(
            "Energy: {energy}    Current expression: {current_expr}\r\n"
        )),
        Print(
            "←/→: move  ↑/↓: switch group  Enter/Space: select  b: bind  e: end turn  c: clear  Esc/q: quit\r\n"
        ),
        Print(format!("{status}\r\n")),
        Print("Deck\r\n")
    )?;
    render_card_group(output, &cards, selected, 0)?;
    queue!(output, Print("\r\n\r\nBindings — one use this turn\r\n"))?;
    render_card_group(output, &turn.sorted_bindings(), selected, cards.len())?;
    output.flush()
}

fn render_card_group(
    output: &mut impl Write,
    cards: &[&CardDefinition],
    selected: usize,
    offset: usize,
) -> io::Result<()> {
    if cards.is_empty() {
        queue!(output, Print("(none)"))?;
        return Ok(());
    }
    let rows: Vec<_> = cards
        .iter()
        .map(|card| card_rows(card, card_width(card)))
        .collect();
    for row_index in 0..CARD_HEIGHT {
        for (card_index, card) in rows.iter().enumerate() {
            if card_index > 0 {
                queue!(output, Print("  "))?;
            }
            if card_index + offset == selected {
                queue!(output, SetAttribute(Attribute::Reverse))?;
            }
            queue!(output, Print(&card[row_index]))?;
            if card_index + offset == selected {
                queue!(output, SetAttribute(Attribute::Reset))?;
            }
        }
        if row_index + 1 < CARD_HEIGHT {
            queue!(output, Print("\r\n"))?;
        }
    }
    Ok(())
}

fn resolve_cards<'a>(library: &'a CardLibrary, deck: &Deck) -> io::Result<Vec<&'a CardDefinition>> {
    deck.cards()
        .iter()
        .map(|instance| {
            library.get(instance.definition()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "deck contains a card missing from the library",
                )
            })
        })
        .collect()
}

fn card_width(card: &CardDefinition) -> usize {
    [
        card.name().chars().count(),
        card.term().category().chars().count(),
        card.scheme().to_string().chars().count(),
    ]
    .into_iter()
    .max()
    .unwrap_or(MINIMUM_INNER_WIDTH)
    .max(MINIMUM_INNER_WIDTH)
}

fn card_rows(card: &CardDefinition, width: usize) -> [String; CARD_HEIGHT] {
    let border = "─".repeat(width);

    [
        format!("┌{border}┐"),
        format!("│{}│", centered(card.name(), width)),
        format!("│{}│", centered(card.term().category(), width)),
        format!("│{}│", centered(&card.scheme().to_string(), width)),
        format!("└{border}┘"),
    ]
}

fn centered(content: &str, width: usize) -> String {
    let padding = width.saturating_sub(content.chars().count());
    let left = padding / 2;
    let right = padding - left;
    format!("{}{content}{}", " ".repeat(left), " ".repeat(right))
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(stdout(), EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            stdout(),
            SetAttribute(Attribute::Reset),
            Show,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::starter_cards;
    use crate::cards::{CardInstance, CardTerm};
    use crate::language::{Operator, Type, TypeScheme, Value};

    #[test]
    fn failed_actions_keep_selected_bindings_recoverable() {
        let (_, mut deck) = starter_cards();
        let mut turn = TurnState::new();
        turn.bind_expression("f", Expr::Operator(Operator::Add))
            .unwrap();
        let original = turn.bindings()["f"].clone();
        let mut builder = ExpressionBuilder::new();
        builder.select_binding("f", &mut turn).unwrap();
        assert!(score_selection(&mut builder, &mut turn).is_err());
        assert!(bind_selection(&mut builder, &mut turn, "bad name").is_err());
        assert_eq!(turn.current_energy(), 2);
        assert!(turn.bindings().is_empty());
        assert_eq!(builder.current_expr(), "f");
        builder.clear(&mut deck, &mut turn);
        assert_eq!(turn.bindings()["f"], original);
        assert_eq!(deck.cards().len(), 4);
    }

    #[test]
    fn binding_a_selected_binding_consumes_the_input_and_keeps_only_the_result() {
        let (_, mut deck) = starter_cards();
        let mut turn = TurnState::new();
        turn.bind_expression("x", Expr::Literal(Value::Int(3)))
            .unwrap();
        let mut builder = ExpressionBuilder::new();
        builder.select_binding("x", &mut turn).unwrap();
        assert!(overwrite_warning(&turn, &builder, "x").is_some());
        bind_selection(&mut builder, &mut turn, "x").unwrap();
        builder.clear(&mut deck, &mut turn);
        assert_eq!(turn.bindings().len(), 1);
        builder.select_binding("x", &mut turn).unwrap();
        bind_selection(&mut builder, &mut turn, "y").unwrap();
        builder.clear(&mut deck, &mut turn);
        assert_eq!(turn.bindings().len(), 1);
        assert!(!turn.bindings().contains_key("x"));
        assert_eq!(
            turn.bindings()["y"].term(),
            &CardTerm::Binding(Expr::Literal(Value::Int(3)))
        );
    }

    #[test]
    fn bind_then_score_consumes_binding_without_registering_persistent_definitions() {
        let (mut library, mut deck) = starter_cards();
        deck.add(CardInstance::new(library.register_operator(Operator::Add)));
        let original_library_size = library.len();
        let mut turn = TurnState::new();
        let mut builder = ExpressionBuilder::new();
        builder.select(0, &mut deck, &library).unwrap();
        builder.select(2, &mut deck, &library).unwrap();
        builder.select(0, &mut deck, &library).unwrap();
        bind_selection(&mut builder, &mut turn, "x").unwrap();
        assert_eq!(builder.current_expr(), "");
        assert_eq!(deck.cards().len(), 2);
        builder.select_binding("x", &mut turn).unwrap();
        builder.select(1, &mut deck, &library).unwrap();
        builder.select(0, &mut deck, &library).unwrap(); // Remaining literal 5.
        assert_eq!(score_selection(&mut builder, &mut turn), Ok(8));
        assert!(turn.bindings().is_empty());
        assert_eq!(turn.current_energy(), 1);
        assert_eq!(library.len(), original_library_size);
        builder.clear(&mut deck, &mut turn);
        assert_eq!(deck.cards().len(), 0);
    }

    #[test]
    fn failed_actions_preserve_selection_for_clearing_or_retry() {
        let (mut library, mut deck) = starter_cards();
        let boolean = library
            .register(
                "true",
                CardTerm::Literal(Value::Bool(true)),
                TypeScheme::monomorphic(Type::Bool),
            )
            .unwrap();
        deck.add(CardInstance::new(boolean));
        let mut builder = ExpressionBuilder::new();
        let mut turn = TurnState::new();
        builder.select(4, &mut deck, &library).unwrap();
        assert!(score_selection(&mut builder, &mut turn).is_err());
        assert!(bind_selection(&mut builder, &mut turn, "bad name").is_err());
        assert_eq!(builder.current_expr(), "true");
        assert_eq!(turn.current_energy(), 3);
        assert!(turn.bindings().is_empty());
        builder.clear(&mut deck, &mut turn);
        assert_eq!(deck.cards().len(), 5);
    }

    #[test]
    fn binding_display_is_sorted_and_overwrite_warning_is_advisory() {
        let (mut library, mut deck) = starter_cards();
        let persistent = library
            .register(
                "a",
                CardTerm::Definition(Expr::Literal(Value::Int(8))),
                TypeScheme::monomorphic(Type::Int),
            )
            .unwrap();
        deck.add(CardInstance::new(persistent));
        let mut turn = TurnState::new();
        turn.bind_expression("z", Expr::Literal(Value::Int(1)))
            .unwrap();
        turn.bind_expression("a", Expr::Literal(Value::Int(2)))
            .unwrap();
        assert_eq!(
            turn.sorted_bindings()
                .iter()
                .map(|card| card.name())
                .collect::<Vec<_>>(),
            vec!["a", "z"]
        );
        assert!(
            overwrite_warning(&turn, &ExpressionBuilder::new(), "a")
                .unwrap()
                .contains("overwrite")
        );
        assert!(overwrite_warning(&turn, &ExpressionBuilder::new(), "other").is_none());
        let mut builder = ExpressionBuilder::new();
        builder.select_binding("a", &mut turn).unwrap();
        bind_selection(&mut builder, &mut turn, "a").unwrap();
        let mut rendered = Vec::new();
        render(
            &mut rendered,
            &library,
            &deck,
            &builder,
            deck.cards().len(),
            "",
            &turn,
        )
        .unwrap();
        let rendered = String::from_utf8(rendered).unwrap();
        assert!(rendered.contains("Bindings — one use this turn"));
        assert!(rendered.contains("b: bind"));
        assert_eq!(
            library.get(persistent).unwrap().term(),
            &CardTerm::Definition(Expr::Literal(Value::Int(8)))
        );
    }

    #[test]
    fn unicode_card_contains_name_category_and_type() {
        let (library, deck) = starter_cards();
        let cards = resolve_cards(&library, &deck).unwrap();
        let rows = card_rows(cards[0], card_width(cards[0]));

        assert_eq!(rows[0], "┌─────────┐");
        assert_eq!(rows[1], "│    1    │");
        assert_eq!(rows[2], "│ Literal │");
        assert_eq!(rows[3], "│   Int   │");
        assert_eq!(rows[4], "└─────────┘");
    }
}
