//! Picks which of a day's readings a chart needs when it asks for fewer
//! points than the day holds.

/// Rows to keep, oldest first, and the time each bucket spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub rows: Vec<usize>,
    pub bucket_seconds: i64,
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    first: usize,
    lowest: Option<(i64, usize)>,
    highest: Option<(i64, usize)>,
}

/// Splits the window into `points / 2` equal buckets and keeps, from each,
/// the rows where the hero line is lowest and highest, or the bucket's first
/// row when none of its rows has a hero value. Each line in `peaked` also
/// keeps the row holding its highest value of the day, so peak markers stay
/// exact. Every row kept is a real reading. Returns `None` when the day
/// already fits in `points` rows.
pub fn select_rows(
    timestamps: &[i64],
    hero: &[Option<i64>],
    peaked: &[Vec<Option<i64>>],
    window: (i64, i64),
    points: usize,
) -> Option<Selection> {
    if timestamps.len() <= points {
        return None;
    }
    let bucket_count = (points / 2).max(1) as i64;
    let (from, to) = window;
    let span = (to - from + 1).max(1);
    let bucket_seconds = (span + bucket_count - 1) / bucket_count;

    let mut buckets: Vec<Option<Bucket>> = vec![None; bucket_count as usize];
    for (row, &timestamp) in timestamps.iter().enumerate() {
        let position = ((timestamp - from) / bucket_seconds).clamp(0, bucket_count - 1) as usize;
        let bucket = buckets[position].get_or_insert(Bucket {
            first: row,
            lowest: None,
            highest: None,
        });
        if let Some(value) = hero[row] {
            if bucket.lowest.map_or(true, |(lowest, _)| value < lowest) {
                bucket.lowest = Some((value, row));
            }
            if bucket.highest.map_or(true, |(highest, _)| value > highest) {
                bucket.highest = Some((value, row));
            }
        }
    }

    let mut rows = Vec::new();
    for bucket in buckets.into_iter().flatten() {
        match (bucket.lowest, bucket.highest) {
            (Some((_, lowest_row)), Some((_, highest_row))) => {
                rows.push(lowest_row);
                rows.push(highest_row);
            }
            _ => rows.push(bucket.first),
        }
    }
    for line in peaked {
        let highest = line
            .iter()
            .enumerate()
            .filter_map(|(row, value)| value.map(|value| (value, row)))
            .fold(None, |best: Option<(i64, usize)>, (value, row)| match best {
                Some((best_value, _)) if best_value >= value => best,
                _ => Some((value, row)),
            });
        if let Some((_, row)) = highest {
            rows.push(row);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    Some(Selection {
        rows,
        bucket_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 14 hour day at 5 second readings, with a sine for the hero.
    fn day(rows: usize) -> (Vec<i64>, Vec<Option<i64>>) {
        let timestamps: Vec<i64> = (0..rows as i64).map(|row| 5 * 3600 + row * 5).collect();
        let hero = (0..rows)
            .map(|row| {
                let progress = row as f64 / rows as f64;
                Some((3000.0 * (std::f64::consts::PI * progress).sin()) as i64)
            })
            .collect();
        (timestamps, hero)
    }

    #[test]
    fn a_day_that_fits_is_left_whole() {
        let (timestamps, hero) = day(10);
        assert_eq!(select_rows(&timestamps, &hero, &[], (0, 86_399), 600), None);
    }

    #[test]
    fn rows_stay_within_points_plus_one_per_peaked_line() {
        let (timestamps, hero) = day(10_080);
        let first_line = hero.clone();
        let second_line: Vec<Option<i64>> = (0..10_080).map(|row| Some((row % 97) as i64)).collect();

        let selection =
            select_rows(&timestamps, &hero, &[first_line, second_line], (0, 86_399), 600).unwrap();

        assert!(selection.rows.len() <= 602, "{}", selection.rows.len());
        assert!(selection.rows.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_hero_high_and_each_peaked_high_survive() {
        let (timestamps, hero) = day(10_080);
        let hero_high = (0..hero.len()).max_by_key(|&row| hero[row]).unwrap();
        let mut spike: Vec<Option<i64>> = vec![Some(2300); 10_080];
        spike[1_234] = Some(2_600);

        let selection = select_rows(&timestamps, &hero, &[spike], (0, 86_399), 600).unwrap();

        assert!(selection.rows.contains(&hero_high));
        assert!(selection.rows.contains(&1_234));
    }

    #[test]
    fn a_bucket_without_hero_values_keeps_its_first_row() {
        let (timestamps, mut hero) = day(10_080);
        for value in hero.iter_mut().take(2_000) {
            *value = None;
        }

        let selection = select_rows(&timestamps, &hero, &[], (0, 86_399), 600).unwrap();

        assert!(selection.rows.contains(&0));
    }

    #[test]
    fn neighbouring_rows_sit_within_two_buckets_on_a_day_without_gaps() {
        // Each even bucket has its lowest and highest readings at its start,
        // each odd bucket at its end, which puts kept rows almost two buckets
        // apart while no reading is missing.
        let bucket_seconds = 288;
        let timestamps: Vec<i64> = (0..10_080).map(|row| 5 * 3600 + row * 5).collect();
        let hero: Vec<Option<i64>> = timestamps
            .iter()
            .map(|&timestamp| {
                let bucket = timestamp / bucket_seconds;
                let offset = timestamp % bucket_seconds;
                let start = if bucket % 2 == 0 { 0 } else { bucket_seconds - 10 };
                Some(match offset - start {
                    0..=4 => 3000,
                    5..=9 => 0,
                    _ => 1500,
                })
            })
            .collect();
        let selection = select_rows(&timestamps, &hero, &[], (0, 86_399), 600).unwrap();
        let longest_step = selection
            .rows
            .windows(2)
            .map(|pair| timestamps[pair[1]] - timestamps[pair[0]])
            .max()
            .unwrap();
        assert!(longest_step > selection.bucket_seconds, "{longest_step}");
        assert!(longest_step <= 2 * selection.bucket_seconds, "{longest_step}");
    }

    #[test]
    fn buckets_split_the_window_evenly() {
        let (timestamps, hero) = day(10_080);
        let selection = select_rows(&timestamps, &hero, &[], (0, 86_399), 600).unwrap();
        assert_eq!(selection.bucket_seconds, 288);
    }
}
