//! tsgo `format/rulesmap.go`: the rules of each pair of tokens, in the
//! order they apply.

use std::sync::OnceLock;

use tsc_syntax::SyntaxKind;

use super::context::FormattingContext;
use super::rule::{Rule, RuleAction};
use super::rules::get_all_rules;

/// The rules and, for each pair of tokens, the indexes of its rules.
struct RulesMap {
    rules: Vec<Rule>,
    buckets: Vec<Vec<usize>>,
}

/// tsgo `getRules`: the rules that apply to the context's tokens, each
/// one's action not excluded by the actions of the rules before it.
pub(super) fn get_rules(context: &FormattingContext<'_>, rules: &mut Vec<&'static Rule>) {
    let map = rules_map();
    let bucket = &map.buckets[get_rule_bucket_index(
        context.current_token_span.kind,
        context.next_token_span.kind,
    )];
    let mut rule_action_mask = RuleAction::NONE;
    'outer: for &index in bucket {
        let rule = &map.rules[index];
        let accept_rule_actions = !get_rule_action_exclusion(rule_action_mask);
        if rule.action.intersects(accept_rule_actions) {
            for predicate in &rule.context {
                if !predicate(context) {
                    continue 'outer;
                }
            }
            rules.push(rule);
            rule_action_mask |= rule.action;
        }
    }
}

fn get_rule_bucket_index(row: SyntaxKind, column: SyntaxKind) -> usize {
    assert!(
        row <= SyntaxKind::LastKeyword && column <= SyntaxKind::LastKeyword,
        "Must compute formatting context from tokens"
    );
    usize::from(row.value()) * MAP_ROW_LENGTH + usize::from(column.value())
}

const MASK_BIT_SIZE: u32 = 5;
const MASK: u32 = 0b11111;
const MAP_ROW_LENGTH: usize = SyntaxKind::LastToken.value() as usize + 1;

/// tsgo `getRuleActionExclusion`: the actions that cannot apply at the same
/// position as `rule_action`.
fn get_rule_action_exclusion(rule_action: RuleAction) -> RuleAction {
    let mut mask = RuleAction::NONE;
    if rule_action.intersects(RuleAction::STOP_PROCESSING_SPACE_ACTIONS) {
        mask |= RuleAction::MODIFY_SPACE_ACTION;
    }
    if rule_action.intersects(RuleAction::STOP_PROCESSING_TOKEN_ACTIONS) {
        mask |= RuleAction::MODIFY_TOKEN_ACTION;
    }
    if rule_action.intersects(RuleAction::MODIFY_SPACE_ACTION) {
        mask |= RuleAction::MODIFY_SPACE_ACTION;
    }
    if rule_action.intersects(RuleAction::MODIFY_TOKEN_ACTION) {
        mask |= RuleAction::MODIFY_TOKEN_ACTION;
    }
    mask
}

fn rules_map() -> &'static RulesMap {
    static RULES_MAP: OnceLock<RulesMap> = OnceLock::new();
    RULES_MAP.get_or_init(build_rules_map)
}

/// tsgo `buildRulesMap`.
fn build_rules_map() -> RulesMap {
    let mut rules = Vec::new();
    let mut buckets = vec![Vec::new(); MAP_ROW_LENGTH * MAP_ROW_LENGTH];
    // Used only while the buckets are built.
    let mut rules_bucket_construction_state_list = vec![0; buckets.len()];
    for spec in get_all_rules() {
        let specific_rule = spec.left_token_range.is_specific && spec.right_token_range.is_specific;
        let rule = rules.len();
        for &left in &spec.left_token_range.tokens {
            for &right in &spec.right_token_range.tokens {
                let index = get_rule_bucket_index(left, right);
                add_rule(
                    &mut buckets[index],
                    rule,
                    &spec.rule,
                    specific_rule,
                    &mut rules_bucket_construction_state_list[index],
                );
            }
        }
        rules.push(spec.rule);
    }
    RulesMap { rules, buckets }
}

/// tsgo `RulesPosition`.
#[derive(Clone, Copy)]
enum RulesPosition {
    StopRulesSpecific = 0,
    StopRulesAny = MASK_BIT_SIZE as isize,
    ContextRulesSpecific = MASK_BIT_SIZE as isize * 2,
    ContextRulesAny = MASK_BIT_SIZE as isize * 3,
    NoContextRulesSpecific = MASK_BIT_SIZE as isize * 4,
    NoContextRulesAny = MASK_BIT_SIZE as isize * 5,
}

/// tsgo `addRule`: a bucket holds its rules in this order:
///
/// 1. Ignore rules with specific token combination
/// 2. Ignore rules with any token combination
/// 3. Context rules with specific token combination
/// 4. Context rules with any token combination
/// 5. Non-context rules with specific token combination
/// 6. Non-context rules with any token combination
///
/// The construction state counts the rules of each sub-bucket in 5 bits, so
/// a rule goes to the end of its sub-bucket.
fn add_rule(
    bucket: &mut Vec<usize>,
    index: usize,
    rule: &Rule,
    specific_tokens: bool,
    construction_state: &mut u32,
) {
    let position = if rule.action.intersects(RuleAction::STOP_ACTION) {
        if specific_tokens {
            RulesPosition::StopRulesSpecific
        } else {
            RulesPosition::StopRulesAny
        }
    } else if !rule.context.is_empty() {
        if specific_tokens {
            RulesPosition::ContextRulesSpecific
        } else {
            RulesPosition::ContextRulesAny
        }
    } else if specific_tokens {
        RulesPosition::NoContextRulesSpecific
    } else {
        RulesPosition::NoContextRulesAny
    };

    let state = *construction_state;
    bucket.insert(get_rule_insertion_index(state, position), index);
    *construction_state = increase_insertion_index(state, position);
}

fn get_rule_insertion_index(mut index_bitmap: u32, mask_position: RulesPosition) -> usize {
    let mut index = 0;
    let mut position = 0;
    while position <= mask_position as u32 {
        index += (index_bitmap & MASK) as usize;
        index_bitmap >>= MASK_BIT_SIZE;
        position += MASK_BIT_SIZE;
    }
    index
}

fn increase_insertion_index(index_bitmap: u32, mask_position: RulesPosition) -> u32 {
    let shift = mask_position as u32;
    let value = ((index_bitmap >> shift) & MASK) + 1;
    assert!(
        value & MASK == value,
        "Adding more rules into the sub-bucket than allowed. Maximum allowed is 32 rules."
    );
    (index_bitmap & !(MASK << shift)) | (value << shift)
}
