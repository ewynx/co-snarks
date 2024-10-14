use super::{CircomGroth16Prover, IoResult};
use ark_ec::pairing::Pairing;
use ark_ff::PrimeField;
use mpc_core::protocols::shamir::{
    arithmetic, core, network::ShamirNetwork, pointshare, ShamirPointShare, ShamirPrimeFieldShare,
    ShamirProtocol,
};
use rayon::prelude::*;

/// A Groth16 dirver unsing shamir secret sharing
///
/// Contains two [`ShamirProtocol`]s, `protocol0` for the main execution and `protocol0` for parts that can run concurrently.
pub struct ShamirGroth16Driver<F: PrimeField, N: ShamirNetwork> {
    protocol0: ShamirProtocol<F, N>,
    protocol1: ShamirProtocol<F, N>,
}

impl<F: PrimeField, N: ShamirNetwork> ShamirGroth16Driver<F, N> {
    /// Create a new [`ShamirGroth16Driver`] with two [`ShamirProtocol`]s
    pub fn new(protocol0: ShamirProtocol<F, N>, protocol1: ShamirProtocol<F, N>) -> Self {
        Self {
            protocol0,
            protocol1,
        }
    }
}

impl<P: Pairing, N: ShamirNetwork> CircomGroth16Prover<P>
    for ShamirGroth16Driver<P::ScalarField, N>
{
    type ArithmeticShare = ShamirPrimeFieldShare<P::ScalarField>;
    type PointShareG1 = ShamirPointShare<P::G1>;
    type PointShareG2 = ShamirPointShare<P::G2>;

    type PartyID = usize;

    fn rand(&mut self) -> IoResult<Self::ArithmeticShare> {
        self.protocol0.rand()
    }

    fn get_party_id(&self) -> Self::PartyID {
        self.protocol0.network.get_id()
    }

    fn evaluate_constraint(
        _party_id: Self::PartyID,
        lhs: &[(P::ScalarField, usize)],
        public_inputs: &[P::ScalarField],
        private_witness: &[Self::ArithmeticShare],
    ) -> Self::ArithmeticShare {
        let mut acc = Self::ArithmeticShare::default();
        for (coeff, index) in lhs {
            if index < &public_inputs.len() {
                let val = public_inputs[*index];
                let mul_result = val * coeff;
                arithmetic::add_assign_public(&mut acc, mul_result);
            } else {
                let current_witness = private_witness[*index - public_inputs.len()];
                arithmetic::add_assign(&mut acc, arithmetic::mul_public(current_witness, *coeff));
            }
        }
        acc
    }

    fn promote_to_trivial_shares(
        _id: Self::PartyID,
        public_values: &[P::ScalarField],
    ) -> Vec<Self::ArithmeticShare> {
        arithmetic::promote_to_trivial_shares(public_values)
    }

    fn local_mul_vec(
        &mut self,
        a: Vec<Self::ArithmeticShare>,
        b: Vec<Self::ArithmeticShare>,
    ) -> Vec<P::ScalarField> {
        arithmetic::local_mul_vec(&a, &b)
    }

    fn mul(
        &mut self,
        r: Self::ArithmeticShare,
        s: Self::ArithmeticShare,
    ) -> IoResult<Self::ArithmeticShare> {
        arithmetic::mul(r, s, &mut self.protocol1)
    }

    fn distribute_powers_and_mul_by_const(
        coeffs: &mut [Self::ArithmeticShare],
        roots: &[P::ScalarField],
    ) {
        coeffs
            .par_iter_mut()
            .zip_eq(roots.par_iter())
            .with_min_len(512)
            .for_each(|(c, pow)| {
                arithmetic::mul_assign_public(c, *pow);
            })
    }

    fn msm_public_points_g1(
        points: &[P::G1Affine],
        scalars: &[Self::ArithmeticShare],
    ) -> Self::PointShareG1 {
        pointshare::msm_public_points(points, scalars)
    }

    fn msm_public_points_g2(
        points: &[P::G2Affine],
        scalars: &[Self::ArithmeticShare],
    ) -> Self::PointShareG2 {
        pointshare::msm_public_points(points, scalars)
    }

    fn scalar_mul_public_point_g1(a: &P::G1, b: Self::ArithmeticShare) -> Self::PointShareG1 {
        pointshare::scalar_mul_public_point(b, a)
    }

    fn add_assign_points_g1(a: &mut Self::PointShareG1, b: &Self::PointShareG1) {
        pointshare::add_assign(a, b)
    }

    fn add_points_g1_half_share(a: Self::PointShareG1, b: &P::G1) -> P::G1 {
        a.inner() + b
    }

    fn add_assign_points_public_g1(_id: Self::PartyID, a: &mut Self::PointShareG1, b: &P::G1) {
        pointshare::add_assign_public(a, b)
    }

    fn open_point_g1(&mut self, a: &Self::PointShareG1) -> IoResult<P::G1> {
        pointshare::open_point(a, &mut self.protocol0)
    }

    fn scalar_mul_g1(
        &mut self,
        a: &Self::PointShareG1,
        b: Self::ArithmeticShare,
    ) -> IoResult<Self::PointShareG1> {
        pointshare::scalar_mul(a, b, &mut self.protocol0)
    }

    fn sub_assign_points_g1(a: &mut Self::PointShareG1, b: &Self::PointShareG1) {
        pointshare::sub_assign(a, b);
    }

    fn scalar_mul_public_point_g2(a: &P::G2, b: Self::ArithmeticShare) -> Self::PointShareG2 {
        pointshare::scalar_mul_public_point(b, a)
    }

    fn add_assign_points_g2(a: &mut Self::PointShareG2, b: &Self::PointShareG2) {
        pointshare::add_assign(a, b)
    }

    fn add_assign_points_public_g2(_id: Self::PartyID, a: &mut Self::PointShareG2, b: &P::G2) {
        pointshare::add_assign_public(a, b)
    }

    fn open_two_points(
        &mut self,
        a: P::G1,
        b: Self::PointShareG2,
    ) -> std::io::Result<(P::G1, P::G2)> {
        let s1 = a;
        let s2 = b.a;
        let (r1, r2) = std::thread::scope(|s| {
            let r1 = s.spawn(|| {
                self.protocol0
                    .network
                    .broadcast_next(s1, self.protocol0.threshold * 2 + 1)
            });
            let r2 = s.spawn(|| {
                self.protocol1
                    .network
                    .broadcast_next(s2, self.protocol0.threshold + 1)
            });
            (r1.join().expect("can join"), r2.join().expect("can join"))
        });
        let r1 = core::reconstruct_point(&r1?, &self.protocol0.open_lagrange_2t);
        let r2 = core::reconstruct_point(&r2?, &self.protocol0.open_lagrange_t);
        Ok((r1, r2))
    }

    fn open_point_and_scalar_mul(
        &mut self,
        g_a: &Self::PointShareG1,
        g1_b: &Self::PointShareG1,
        r: Self::ArithmeticShare,
    ) -> super::IoResult<(P::G1, Self::PointShareG1)> {
        std::thread::scope(|s| {
            let opened = s.spawn(|| pointshare::open_point(g_a, &mut self.protocol0));
            let mul_result = pointshare::scalar_mul(g1_b, r, &mut self.protocol1)?;
            Ok((opened.join().expect("can join")?, mul_result))
        })
    }
}