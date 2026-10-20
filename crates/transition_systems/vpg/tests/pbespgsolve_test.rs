//! Compares our parity-game solver against mCRL2's `pbespgsolve` on random games written in the
//! textual PGSolver `.pg` format.
//!
//! Gated on `MCRL2_PATH`: skipped in the default test run (when the variable is unset) and
//! exercised by the dedicated mCRL2 CI job. Set `MERC_DUMP` to keep the generated `.pg` files for a
//! failing seed.

use std::path::Path;
use std::process::Command;

use merc_io::temp_dir;
use merc_utilities::random_test;
use merc_vpg::Player;
use merc_vpg::random_parity_game;
use merc_vpg::solve_zielonka;
use merc_vpg::write_pg;

/// Compares the winner of the initial vertex (index 0) computed by our Zielonka solver against
/// `pbespgsolve` on random total parity games.
#[test]
#[cfg_attr(miri, ignore)]
fn test_mcrl2_pbespgsolve_vs_zielonka() {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let pbespgsolve = Path::new(&mcrl2_path).join("pbespgsolve");

    random_test(100, |rng| {
        // A total game with a modest size and priority range keeps pbespgsolve fast while still
        // producing a variety of winner partitions. Zielonka requires the game to be total.
        let game = random_parity_game(rng, true, 50, 5, 3);

        let dir = temp_dir("pbespgsolve").unwrap();
        let input_path = dir.path().join("input.pg");
        {
            let mut file = std::fs::File::create(&input_path).unwrap();
            write_pg(&mut file, &game).unwrap();
        }

        // pbespgsolve solves the game and reports whether the initial vertex (index 0) is won by
        // player Even (player 0): "true" means Even wins, "false" means Odd wins. This matches the
        // PGSolver/mCRL2 convention where owner 0 is the player favouring even priorities.
        let output = Command::new(&pbespgsolve)
            .arg(&input_path)
            .output()
            .expect("Failed to run pbespgsolve");
        assert!(
            output.status.success(),
            "pbespgsolve failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mcrl2_even_wins = if stdout.contains("true") {
            true
        } else if stdout.contains("false") {
            false
        } else {
            panic!("Unexpected pbespgsolve output: {stdout:?}");
        };

        // Our solver: the initial vertex is won by Even iff it lies in the Even winning set.
        let (solution, _) = solve_zielonka(&game, false);
        let our_even_wins: bool = solution[Player::Even.to_index()][0];

        assert_eq!(
            our_even_wins, mcrl2_even_wins,
            "Winner of the initial vertex differs from pbespgsolve",
        );
    });
}
