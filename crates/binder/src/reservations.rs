//! The parser-sized identities used by either a loader or a bind driver.
use tsc_syntax::SourceFile;
use tsc_types::identity::direct::Rooms;
use tsc_types::{IdentityDomain, IdentityError, IdentityLease, IdentitySpace};

/// One source's symbols and private-name serials, before its bind starts.
#[derive(Clone, Debug)]
pub struct BindReservations {
    pub symbols: IdentityLease,
    pub serials: IdentityLease,
}

impl BindReservations {
    pub fn counts(source: &SourceFile) -> [u32; 2] {
        [
            source.arena.symbol_bound().max(1),
            u32::try_from(source.arena.nodes().len())
                .expect("source node count exceeds u32")
                .max(1),
        ]
    }

    /// The ordinary driver's ordered reservation, including its existing
    /// first-fit reuse of released ranges.
    pub fn lease(source: &SourceFile, domain: &IdentityDomain) -> Result<Self, IdentityError> {
        let [symbols, serials] = Self::counts(source);
        domain
            .lease_batch(&[
                (IdentitySpace::Symbol, symbols),
                (IdentitySpace::PrivateNameSerial, serials),
            ])
            .map(Self::from_leases)
    }

    /// Exact source views of a stripe already leased in input order.
    pub fn finish_entry(rooms: &mut Rooms, counts: [u32; 2]) -> Result<Self, IdentityError> {
        rooms.finish_entry(&counts).map(Self::from_leases)
    }

    fn from_leases(leases: Vec<IdentityLease>) -> Self {
        let mut leases = leases.into_iter();
        let symbols = leases.next().expect("symbol reservation");
        let serials = leases.next().expect("private-name serial reservation");
        assert_eq!(symbols.space(), IdentitySpace::Symbol);
        assert_eq!(serials.space(), IdentitySpace::PrivateNameSerial);
        assert!(leases.next().is_none());
        Self { symbols, serials }
    }
}
