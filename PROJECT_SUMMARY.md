# Compose the Function

## Project status

`compose-the-function` is an early Rust prototype of a deckbuilder/roguelike in which the player constructs real functional-language expressions from cards. The project currently focuses on the smallest playable terminal loop: select cards, arrange them into an expression, validate the expression, and either bind it as a single-use turn-local card or evaluate it for a score.

The project is intentionally not yet a complete deckbuilder, parser, or Hindley–Milner type system.

Current starter deck:

```text
1 : Int
2 : Int
5 : Int
+ : Int -> Int -> Int
```

Current technology:

- Rust 2024 edition
- Crossterm 0.29 for terminal rendering and keyboard events
- No other runtime dependencies

## Product vision

The long-term idea is a small deckbuilder/roguelike where computational pieces are cards and constructing valid programs is the central mechanic. The cards are not merely programming-themed: they denote terms in an actual language representation, and submitted expressions are type-checked and evaluated.

The intended run structure takes inspiration from games such as *Balatro* and *Slay the Spire*, while scoring comes from evaluated programs.

A representative future turn might look like:

```text
let x = 1
let y = 2
x + y
```

Intermediate assignments consume energy but can make a more valuable final expression possible. Turn-local bindings disappear when the turn ends.

## Current gameplay loop

The executable runs one turn:

1. Initialize the persistent card library and player-owned deck.
2. Create a fresh `TurnState` with three energy and an empty binding registry.
3. Select deck cards or single-use binding cards to assemble an expression.
4. Press **Bind** to type-check and register the expression as a turn-local card, spending one energy.
5. Continue constructing expressions; each binding can be selected once before it is consumed.
6. Press **End turn** to evaluate an integer score, spending one energy and ending the turn.
7. Drop the turn state on scoring, quitting, or an I/O failure.

Registration stores the expression, not an evaluated value. Selecting a binding removes it from the workspace and captures its expression in the construction. Rebinding an existing name replaces its expression and inferred type for subsequent selections. Previously selected expressions and previously constructed bindings retain their old contents.

Controls:

```text
Left/Right    Navigate within the current group, wrapping at its ends
Up/Down       Switch to deck/bindings at index 0; no-op if destination is empty
Enter/Space  Select a deck card or a single-use binding
b            Bind the current expression
e            End the turn and score the current expression
c            Clear the current expression
Esc/q        Quit
```

Binding-name input stays inside the terminal interface:

```text
Type          Add characters
Backspace     Delete the last character
Enter         Confirm, including overwriting an existing binding
Esc           Cancel binding
```

The name-entry UI shows an advisory overwrite warning as soon as the entered name matches an available or currently selected binding. No additional confirmation is required. Names are local to the turn and may also match persistent card names; selection distinguishes the two sources.

Invalid bindings and failed scoring leave energy, bindings, and the current selection unchanged. Clear returns selected deck instances and restores selected bindings. Successful actions consume both kinds of input; they are not added back by later clearing.

## Terminal interface

Cards are rendered with Unicode box-drawing characters. Each card displays its surface name, category, and type:

```text
┌─────────┐  ┌─────────────────┐
│    1    │  │        +        │
│ Literal │  │    Operator     │
│   Int   │  │Int -> Int -> Int│
└─────────┘  └─────────────────┘
```

Deck cards and bindings are displayed in separate groups. The binding group is labeled “Bindings — one use this turn” and sorted alphabetically. The selected card uses reversed terminal colors. Every card is sized independently so an operator's longer type does not unnecessarily widen literal cards.

The interface uses:

- raw mode for immediate key events;
- an alternate terminal screen to avoid filling terminal scrollback;
- full-frame redraws after input;
- a guard that restores normal terminal mode, cursor visibility, and the main screen when the selector exits.

The current expression and energy are shown above the available cards:

```text
Energy: 3    Current expression: 1 + 2
```

## Architecture

```text
main.rs
  │
  ├── creates CardLibrary, Deck, and Player
  │
  ├── selector.rs ─────── terminal interaction and rendering
  │       │
  │       └── expression_builder.rs ── selected cards → AST
  │
  ├── player.rs ───────── player deck, turn bindings, energy, and scoring
  │
  ├── cards.rs ────────── card definitions, instances, library, and deck
  │
  └── language.rs ─────── types, values, AST, inference, and evaluation
```

### `src/main.rs`

The binary initializes the persistent library and player, then calls `construct_expression(&library, player.deck_mut())`. The selector owns one turn and returns either `BuildAction::Scored(i64)` or `BuildAction::Quit`. The binary prints a successful score.

### `src/lib.rs`

The library root exposes:

```rust
pub mod cards;
pub mod expression_builder;
pub mod language;
pub mod player;
pub mod selector;
```

### `src/cards.rs`

This module contains the persistent card model.

#### Card definitions

`CardDefinition` stores shared card metadata without an embedded identity:

```rust
pub struct CardDefinition {
    name: String,
    term: CardTerm,
    scheme: TypeScheme,
}

pub enum CardTerm {
    Literal(Value),
    Operator(Operator),
    Binding(Expr),
    Definition(Expr),
}
```

The term variant determines the UI category. A Binding and a Definition both contain expressions; their registration and ownership differ. Binding registration does not evaluate the expression and does not restrict it to a literal.

#### Library, deck, and binding separation

```text
CardLibrary
└── HashMap<CardId, CardDefinition>      persistent templates

Deck
└── Vec<CardInstance>
        └── CardId                      persistent template reference

TurnState
└── HashMap<String, CardDefinition>      temporary named bindings
```

Persistent IDs live in library keys and deck instances. Bindings use their turn-local names and receive no persistent `CardId`.

`CardLibrary::register` returns `Result<CardId, CardRegistrationError>`. It rejects `CardTerm::Binding(_)` with `BindingMustBeTurnLocal` before changing the library. Integer and operator convenience registration still return `CardId`.

Card IDs are allocated using `definitions.len() + 1`. The library has no deletion operation. Adding persistent deletion later requires an independent monotonic allocator; expiring bindings never deletes entries from this library.

`CardDefinition::new` is crate-visible so the turn registry can construct the same metadata shape.

### `src/language.rs`

This module contains the language representation and the currently supported semantics.

#### Types

```rust
pub enum Type {
    Int,
    Bool,
    Var(TypeVarId),
    Arrow(Box<Type>, Box<Type>),
}
```

Function types use recursive arrows. For example:

```text
Int -> Int -> Int
```

means:

```text
Int -> (Int -> Int)
```

`TypeScheme` contains a type and a list of quantified type variables:

```rust
pub struct TypeScheme {
    pub quantified: Vec<TypeVarId>,
    pub typ: Type,
}
```

Only `TypeScheme::monomorphic` is currently used. Generalization and instantiation are not implemented.

#### Values

```rust
pub enum Value {
    Int(i64),
    Bool(bool),
}
```

#### Operators

The operator enum represents semantics, while `Operator::symbol()` provides surface syntax:

```rust
Add                 // +
Subtract            // -
Multiply            // *
Equal               // ==
GreaterThan         // >
GreaterThanOrEqual  // >=
LessThan            // <
LessThanOrEqual     // <=
```

Arithmetic operators have type:

```text
Int -> Int -> Int
```

Comparison operators have type:

```text
Int -> Int -> Bool
```

Operator precedence is currently:

```text
comparisons       1
+ and -           2
*                 3
```

Operators of equal precedence associate to the left in the expression builder.

#### AST

```rust
pub enum Expr {
    Literal(Value),
    Variable(String),
    Lambda {
        parameter: String,
        body: Box<Expr>,
    },
    Apply(Box<Expr>, Box<Expr>),
    Operator(Operator),
}
```

Operators are treated as curried functions. The surface expression:

```text
1 + 2
```

becomes conceptually:

```text
((+) 1) 2
```

and is represented as:

```rust
Expr::Apply(
    Box::new(Expr::Apply(
        Box::new(Expr::Operator(Operator::Add)),
        Box::new(Expr::Literal(Value::Int(1))),
    )),
    Box::new(Expr::Literal(Value::Int(2))),
)
```

Using ordinary application for operators keeps the AST aligned with the functional-language design instead of introducing a separate binary-expression node.

### `src/expression_builder.rs`

`ExpressionBuilder` owns display text and ordered semantic selections. The UI string is not parsed.

Selecting a deck card resolves its persistent definition, removes the exact instance from the deck, and stores that instance with its semantic expression part. Selecting a binding by name removes its card from the turn registry and records it alongside its expression operand in the builder. A second selection of that name fails while it is unavailable.

`SelectedCard::source` distinguishes a deck instance from an owned binding card. `clear(deck, turn)` restores each source to its owner; `commit()` consumes both after a successful action. Restoration never overwrites a newer binding with the same name.

`build()` validates alternating operands and operators and uses a shunting-yard-style reduction with precedence and left associativity. Operators reduce to two nested `Expr::Apply` nodes. For example, `1 + 2 * 5` becomes `1 + (2 * 5)` and evaluates to `11`.

Bindings and persistent Definitions are atomic operands containing their stored ASTs. This preserves grouping and snapshots: if `y` is built using `x`, later rebinding `x` does not rewrite `y`.

Unknown binding names fail without changing the construction. Arbitrary function application and standalone operator selection as a complete expression remain limitations of the current infix builder; the registration API can already store operator and partial-application expressions accepted by inference.

### `src/player.rs`

`Player` owns the deck. Turn energy and bindings live in a separate context:

```rust
pub struct TurnState {
    current_energy: u32,
    bindings: HashMap<String, CardDefinition>,
}
```

`TurnState::new()` starts with `STARTING_ENERGY = 3` and no bindings.

`TurnState::bind_expression(name, expression)`:

1. Checks energy and identifier syntax.
2. Infers the expression's monomorphic type without evaluating it.
3. Inserts or replaces a card with `CardTerm::Binding(expression)` and the inferred scheme.
4. Spends one energy.

All validation happens before mutation. A failed replacement leaves the previous binding intact. Successful replacement may change the binding's type.

`bindings()` exposes an immutable registry view. `sorted_bindings()` provides alphabetical display order. There is no duplicate-name error or check against persistent library names.

`TurnState::evaluate(expression)` checks energy, infers and evaluates the expression, requires an integer score, and spends one energy on success. The coordinator ends the turn after success. Failed scoring does not mutate the turn.

### `src/selector.rs`

`construct_expression(library, deck)` owns a fresh turn and expression builder. Its terminal loop performs bindings and scoring while retaining the same construction on errors.

```rust
pub enum BuildAction {
    Scored(i64),
    Quit,
}
```

The selector commits the builder only after a successful binding or score. On quit or I/O failure it returns uncommitted deck instances before dropping the turn. New bindings remain available until selected or until the turn ends.

The name-entry screen warns about overwriting an existing turn-local binding and accepts Enter normally. Escape cancels without changing the binding or selection. Persistent cards with the same display name remain separate choices.

## Card lifecycle

- **Deck selection:** Remove a specific instance and retain it in the builder.
- **Binding selection:** Remove the binding card from the workspace and retain it in the builder.
- **Clear:** Return uncommitted deck instances and restore selected bindings without spending energy.
- **Cancel turn:** Restore uncommitted inputs, then discard all turn-local bindings.
- **Successful bind:** Register or replace one turn-local card, spend energy, and consume all selected deck cards and binding cards.
- **Successful score:** Evaluate an integer, spend energy, commit the input selection, and drop the turn.
- **Turn expiry:** Discard all bindings without changing the persistent library.

Successful construction still spends ordinary input cards from the current deck. Draw/discard piles and restoring ordinary cards for subsequent turns are separate, unimplemented gameplay systems.

## Monomorphic type inference currently implemented

The current inference function handles:

- integer literals as `Int`;
- Boolean literals as `Bool`;
- operator terms using their declared arrow type;
- function application when the function has an arrow type and the argument exactly matches its input type.

Application inference follows:

```text
function : A -> B
argument : A
----------------
application : B
```

For `1 + 2`:

```text
+       : Int -> Int -> Int
(+) 1   : Int -> Int
((+) 1) 2 : Int
```

Type equality is currently structural equality. There is no substitution or unification algorithm.

## Evaluation currently implemented

The evaluator supports:

- literal values;
- fully applied binary operator expressions in the AST shape produced by `ExpressionBuilder`;
- checked integer addition, subtraction, and multiplication;
- integer equality and ordering comparisons.

Integer overflow returns `EvaluationError::IntegerOverflow` instead of wrapping.

The evaluator recognizes fully applied operators by matching:

```text
Apply(
    Apply(Operator(op), left),
    right,
)
```

It evaluates both operands and applies the operator.

## What is implemented

- Persistent literal, operator, and expression definition metadata, separate from deck instances
- A separate turn-owned registry of expression-backed Binding cards
- Type-checked registration without eager evaluation, including supported function-typed expressions
- Rebinding with updated type metadata and an advisory overwrite warning
- Single-use binding selection with clear-to-restore behavior and stable alphabetical display
- Turn cleanup on scoring, quitting, and I/O failure
- Transactional validation: failed actions preserve energy, bindings, and selections
- Unicode terminal cards, navigation, binding name entry, and inline errors
- Free-order selection with validation on Bind or End turn
- Infix expression construction with operator precedence and left associativity
- Curried operator AST representation and monomorphic inference
- Checked arithmetic, comparisons, and integer scoring
- Unit tests for language, cards, registration, rebinding, construction, action handling, and rendering

## What is represented but not implemented

The following concepts exist in data types but do not yet have complete semantics:

- `Expr::Variable`
- `Expr::Lambda`
- general function application
- `Type::Var`
- quantified `TypeScheme`s

They are structural preparation for later language work.

## What is not implemented

### Runtime environments and richer turn progression

Turn-local ownership is implemented, but runtime variable environments are not. Bindings currently expand their stored expressions into the builder; `Expr::Variable` does not resolve names through an environment.

There are no eager runtime values, lazy thunks, or memoization caches attached to bindings. General function values and captured environments remain future evaluator work.

The executable handles one turn. New `TurnState` instances reset bindings and energy, but there is no multi-turn coordinator, draw/discard flow, or deck replenishment. Spending all three energy on bindings still leaves no energy for scoring; energy-budget design is unchanged.

### Full type inference

Not implemented:

- variable lookup through a type environment;
- fresh inference-variable generation;
- substitutions;
- occurs checks;
- unification;
- lambda inference;
- general application inference involving unknown types;
- let generalization;
- scheme instantiation;
- let-polymorphism;
- friendly type-error formatting.

Type variables are currently representational scaffolding. A source variable such as `x` is not itself a `TypeVarId`. Once environments exist, `let x = 5` should store `x -> Int` in the turn's type environment.

### Full evaluation

Not implemented:

- variable lookup through a value environment;
- closures;
- lambda evaluation;
- general function application;
- partial application as a runtime value;
- captured environments;
- evaluation strategies or reduction limits.

### Richer expression syntax

The current builder accepts only an alternating infix sequence of operands and binary operators. It does not support:

- parentheses cards;
- unary operators;
- prefix function application from selected cards;
- lambda construction;
- explicit `let` syntax inside expressions;
- conditional expressions;
- manual text entry or parsing.

### Derived-card safety

There is no free-variable analysis. The intended initial rule is that persistent player-created definitions must be closed, but the code does not yet check this.

### Deckbuilder and roguelike systems

Not implemented:

- draw piles, hands, and discard piles;
- shuffling;
- card rewards or shops;
- encounters and enemy behavior;
- relic-like modifiers;
- upgrades or per-instance modifiers;
- scoring multipliers;
- run progression;
- saving/loading;
- deck/library persistence;
- turn and round reset behavior;
- computational constraints or reduction-cost mechanics.

## Important current semantics and caveats

1. The UI string is display text; selected semantic parts build the AST.
2. Invalid construction order is allowed until Bind or End turn.
3. Deck instances are removed when selected. Clear returns them at the end of the deck.
4. Binding selection reserves the card. Successful Bind or End turn consumes it; Clear restores it.
5. Rebinding replaces a name for later selections; earlier snapshots retain their old meanings.
6. Binding registration does not evaluate the expression. Runtime errors can therefore surface when scoring.
7. Only integers can score. Function-typed expressions may be registered through the API even when the builder or evaluator cannot yet use them.
8. Binding names are independent of persistent card names and IDs.
9. The persistent library has no deletion operation; its ID allocator assumes it only grows.
10. Expiring bindings does not yet implement a complete deckbuilder turn-reset system.

## Recommended next structural step

The turn-owned registry is in place. Future language work can extend general application, variable lookup, lambdas, and unification. Any move from current expression expansion to evaluated bindings, closures, or lazy evaluation needs an explicit semantics decision.

Persistent player-created Definitions remain a separate upgrade system. Their eventual registration should enforce the intended closed-expression rule; ordinary binding does not create permanent upgrades.

## Tests and code health

The current suite contains 27 passing unit tests. Coverage includes:

- Persistent registration rejects temporary bindings without consuming an ID.
- Bindings preserve expressions and inferred types, including operators and partial applications.
- Rebinding updates expressions/types; invalid or unaffordable replacements preserve old state.
- A selected binding cannot be selected again; committed bindings stay consumed.
- Previously selected expressions and derived bindings preserve their meaning after rebinding.
- Clear returns deck instances and bindings without duplication or overwriting newer bindings.
- Failed binding/scoring actions preserve selections for retry or clear.
- Fresh turns start without old bindings and with reset energy.
- Binding display order, advisory warnings, and names shared with persistent cards.
- Existing language inference, evaluation, precedence, and Unicode rendering behavior.

Validation:

```sh
cargo test --offline
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings
```

## Scope summary

The prototype now supports typed expression construction, single-use turn-local Binding cards, rebinding, and integer scoring. Persistent Definitions remain separate library entries for a future deckbuilding upgrade mechanic. The project is still a one-turn terminal prototype, without a complete functional language or roguelike progression system.
