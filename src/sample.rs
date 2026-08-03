/// Build deterministic, internally consistent fictional precinct CSV data.
pub fn sample_csv(rows: usize, seed: u64) -> String {
    let mut output = String::from(
        "Jurisdiction,Precinct,Registered_Voters,Ballots_Cast,Valid_Contest_Votes,Votes_Candidate_A,Votes_Candidate_B,Votes_Down_Ballot_A,Votes_Down_Ballot_B,Write_In_Votes,Undervotes,Overvotes,Latitude,Longitude,Reported_Turnout_Percent,Source_Note\n",
    );
    let mut state = seed.max(1);
    for index in 0..rows {
        let random = |state: &mut u64| {
            *state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((*state >> 33) as f64) / ((1_u64 << 31) as f64)
        };
        let registered = 500 + (random(&mut state) * 2500.0) as u64;
        let turnout_rate = (0.52 + random(&mut state) * 0.28).clamp(0.35, 0.92);
        let ballots = (registered as f64 * turnout_rate).floor() as u64;
        let undervotes = (ballots as f64 * (0.006 + random(&mut state) * 0.012)).floor() as u64;
        let overvotes = (ballots as f64 * random(&mut state) * 0.002).floor() as u64;
        let valid = ballots.saturating_sub(undervotes + overvotes);
        let jurisdiction = index % 6;
        let share = (0.42 + jurisdiction as f64 * 0.018 + (random(&mut state) - 0.5) * 0.08)
            .clamp(0.15, 0.85);
        let candidate_a = (valid as f64 * share).round() as u64;
        let candidate_b = valid - candidate_a;
        let down_a = candidate_a.saturating_sub((candidate_a as f64 * 0.025).round() as u64);
        let down_b = candidate_b.saturating_sub((candidate_b as f64 * 0.020).round() as u64);
        let latitude = 42.0 + (index / 20) as f64 * 0.08 + (random(&mut state) - 0.5) * 0.01;
        let longitude = -85.0 + (index % 20) as f64 * 0.08 + (random(&mut state) - 0.5) * 0.01;
        let turnout_percent = 100.0 * ballots as f64 / registered as f64;
        output.push_str(&format!(
            "Michigan County {},Precinct {:04},{registered},{ballots},{valid},{candidate_a},{candidate_b},{down_a},{down_b},0,{undervotes},{overvotes},{latitude:.6},{longitude:.6},{turnout_percent:.4},Fictional demonstration data; not official election results\n",
            jurisdiction + 1,
            index + 1,
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_is_deterministic() {
        assert_eq!(sample_csv(3, 42), sample_csv(3, 42));
        assert_eq!(sample_csv(3, 42).lines().count(), 4);
    }
}
