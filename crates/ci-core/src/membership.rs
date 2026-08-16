use core::marker::PhantomData;

use crate::{CompleteMembership, PendingMembership};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MembershipError {
    Missing { index: usize },
    Unexpected { index: usize },
    Duplicate { index: usize },
    Unsorted { index: usize },
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompleteAdapterInput<I, V> {
    membership: CompleteMembership<I, V>,
    values: Box<[(I, V)]>,
}

impl<I, V> CompleteAdapterInput<I, V> {
    pub fn values(&self) -> &[(I, V)] {
        &self.values
    }

    pub fn membership(&self) -> &CompleteMembership<I, V> {
        &self.membership
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompleteCompositeInput<I, V> {
    inner: CompleteAdapterInput<I, V>,
    marker: PhantomData<fn() -> (I, V)>,
}

impl<I, V> CompleteCompositeInput<I, V> {
    pub fn values(&self) -> &[(I, V)] {
        self.inner.values()
    }
}

pub fn complete_adapter_input<I, V>(
    pending: &PendingMembership<I, V>,
    mut values: Vec<(I, V)>,
) -> Result<CompleteAdapterInput<I, V>, MembershipError>
where
    I: Clone + Ord,
{
    validate_values(pending.expected(), &mut values)?;
    Ok(CompleteAdapterInput {
        membership: CompleteMembership::sealed(),
        values: values.into_boxed_slice(),
    })
}

pub fn complete_composite_input<I, V>(
    pending: &PendingMembership<I, V>,
    values: Vec<(I, V)>,
) -> Result<CompleteCompositeInput<I, V>, MembershipError>
where
    I: Clone + Ord,
{
    Ok(CompleteCompositeInput {
        inner: complete_adapter_input(pending, values)?,
        marker: PhantomData,
    })
}

fn validate_values<I, V>(expected: &[I], values: &mut [(I, V)]) -> Result<(), MembershipError>
where
    I: Ord,
{
    if values.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        let index = values
            .windows(2)
            .position(|pair| pair[0].0 >= pair[1].0)
            .map_or(0, |index| index + 1);
        return if values[index - 1].0 == values[index].0 {
            Err(MembershipError::Duplicate { index })
        } else {
            Err(MembershipError::Unsorted { index })
        };
    }
    if values.len() < expected.len() {
        return Err(MembershipError::Missing {
            index: values.len(),
        });
    }
    if values.len() > expected.len() {
        return Err(MembershipError::Unexpected {
            index: expected.len(),
        });
    }
    for (index, (actual, _)) in values.iter().enumerate() {
        if actual != &expected[index] {
            return Err(if actual < &expected[index] {
                MembershipError::Missing { index }
            } else {
                MembershipError::Unexpected { index }
            });
        }
    }
    Ok(())
}
