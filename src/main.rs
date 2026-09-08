use std::io;

use compose_the_function::{
    cards::starter_cards,
    player::Player,
    selector::{BuildAction, construct_expression},
};

fn main() -> io::Result<()> {
    let (library, deck) = starter_cards();
    let mut player = Player::new(deck);

    println!("Compose the Function");
    match construct_expression(&library, player.deck_mut())? {
        BuildAction::Scored(score) => println!("Score: {score}"),
        BuildAction::Quit => {}
    }
    Ok(())
}
