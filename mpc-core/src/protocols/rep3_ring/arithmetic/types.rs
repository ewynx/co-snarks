use crate::protocols::{
    rep3::{
        id::PartyID,
        network::{IoContext, Rep3Network},
    },
    rep3_ring::ring::{int_ring::IntRing2k, ring_impl::RingElement},
};
use num_traits::Zero;
use rand::{distributions::Standard, prelude::Distribution};
use serde::{Deserialize, Serialize};

/// This type represents a replicated shared value. Since a replicated share of a ring element contains additive shares of two parties, this type contains two ring elements.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(bound = "")]
pub struct Rep3RingShare<T: IntRing2k> {
    /// Share of this party
    pub a: RingElement<T>,
    /// Share of the prev party
    pub b: RingElement<T>,
}

impl<T: IntRing2k> Default for Rep3RingShare<T> {
    fn default() -> Self {
        Self::zero_share()
    }
}

impl<T: IntRing2k> Rep3RingShare<T> {
    /// Constructs the type from two additive shares.
    pub fn new(a: T, b: T) -> Self {
        Self {
            a: RingElement(a),
            b: RingElement(b),
        }
    }

    pub fn new_ring(a: RingElement<T>, b: RingElement<T>) -> Self {
        Self { a, b }
    }

    /// Constructs a zero share.
    pub fn zero_share() -> Self {
        Self {
            a: RingElement::zero(),
            b: RingElement::zero(),
        }
    }

    /// Unwraps the type into two additive shares.
    pub fn ab(self) -> (RingElement<T>, RingElement<T>) {
        (self.a, self.b)
    }

    /// Double the share in place
    pub fn double(&mut self) {
        self.a <<= 1;
        self.b <<= 1;
    }

    /// Generate a random share
    pub fn rand<N: Rep3Network>(io_context: &mut IoContext<N>) -> Self
    where
        Standard: Distribution<T>,
    {
        let (a, b) = io_context.rngs.rand.random_elements();
        Self::new(a, b)
    }

    /// Promotes a public ring element to a replicated share by setting the additive share of the party with id=0 and leaving all other shares to be 0. Thus, the replicated shares of party 0 and party 1 are set.
    pub fn promote_from_trivial(val: &RingElement<T>, id: PartyID) -> Self {
        match id {
            PartyID::ID0 => Self::new_ring(*val, RingElement::zero()),
            PartyID::ID1 => Self::new_ring(RingElement::zero(), *val),
            PartyID::ID2 => Self::zero_share(),
        }
    }
}