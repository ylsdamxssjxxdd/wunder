use super::{ParsedVirtualLog, VirtualReplayTurn};
use anyhow::{anyhow, Result};

pub(super) fn normalize(turns: &mut [VirtualReplayTurn]) -> Result<()> {
    // Stable sorting preserves file order for legacy logs without model-round IDs.
    turns.sort_by_key(|turn| turn.source_round);
    let mut start = 0;
    while start < turns.len() {
        let round = turns[start].source_round;
        let count = turns[start..].partition_point(|turn| turn.source_round == round);
        let group = &mut turns[start..start + count];
        let numbered = group
            .iter()
            .filter(|turn| turn.source_model_round.is_some())
            .count();
        if numbered == 0 {
            for (index, turn) in group.iter_mut().enumerate() {
                turn.source_model_round = Some(index + 1);
            }
        } else if numbered != count {
            return Err(anyhow!(
                "virtual llm replay has mixed model-round numbering at user round {round}"
            ));
        }
        group.sort_by_key(|turn| turn.source_model_round);
        if group
            .windows(2)
            .any(|pair| pair[0].source_model_round == pair[1].source_model_round)
        {
            return Err(anyhow!(
                "virtual llm replay has duplicate model rounds at user round {round}"
            ));
        }
        start += count;
    }
    Ok(())
}

pub(super) fn select(
    parsed: &ParsedVirtualLog,
    user_round: usize,
    model_round: usize,
) -> Result<&VirtualReplayTurn> {
    // Never wrap or fall back: an earlier output may contain a destructive tool call.
    let key = (user_round, Some(model_round));
    let index = parsed.turns.binary_search_by_key(&key, |turn| {
        (turn.source_round, turn.source_model_round)
    }).map_err(|_| anyhow!(
        "virtual llm replay round not found or exhausted (user_round={user_round}, model_round={model_round})"
    ))?;
    Ok(&parsed.turns[index])
}
