use rrising_core::ordinary_movement::{update_ordinary_movement, OrdinaryMovement};

#[test]
fn matches_original_server_ordinary_branch() {
    let mut cases = 0;
    for line in include_str!("../../../evidence/oracles/do_movement.tsv")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let v: Vec<_> = line.split('\t').collect();
        assert_eq!(v.len(), 13);
        let f = |i: usize| f32::from_bits(u32::from_str_radix(v[i], 16).unwrap());
        let mut state = OrdinaryMovement {
            velocity: [7., 8., 9.],
            move_velocity: 123.,
            translation: [1., 2., 3.],
        };
        update_ordinary_movement(&mut state, [f(1), f(2)], f(3), f(4), f(5), v[0] == "1");
        let actual = state
            .velocity
            .into_iter()
            .chain([state.move_velocity])
            .chain(state.translation);
        for (i, actual) in actual.enumerate() {
            assert_eq!(
                actual.to_bits(),
                f(i + 6).to_bits(),
                "case {cases}, output {i}"
            );
        }
        cases += 1;
    }
    assert_eq!(cases, 2240);
}
