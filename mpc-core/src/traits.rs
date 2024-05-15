use ark_ec::{pairing::Pairing, CurveGroup};

use ark_ff::PrimeField;
use ark_poly::EvaluationDomain;
use serde::{de::DeserializeOwned, Serialize};

/// A trait encompassing basic operations for MPC protocols over prime fields.
pub trait PrimeFieldMpcProtocol<F: PrimeField> {
    type FieldShare: Default + Clone;
    type FieldShareVec: 'static
        + for<'a> From<Self::FieldShareSliceMut<'a>>
        + From<Vec<Self::FieldShare>>
        + Clone
        + Serialize
        + DeserializeOwned;
    type FieldShareSlice<'a>: Copy + From<&'a Self::FieldShareVec>;
    type FieldShareSliceMut<'a>: From<&'a mut Self::FieldShareVec>;

    fn add(&mut self, a: &Self::FieldShare, b: &Self::FieldShare) -> Self::FieldShare;
    fn sub(&mut self, a: &Self::FieldShare, b: &Self::FieldShare) -> Self::FieldShare;
    fn add_with_public(&mut self, a: &F, b: &Self::FieldShare) -> Self::FieldShare;
    fn sub_assign_vec(
        &mut self,
        a: &mut Self::FieldShareSliceMut<'_>,
        b: &Self::FieldShareSlice<'_>,
    );
    fn mul(
        &mut self,
        a: &Self::FieldShare,
        b: &Self::FieldShare,
    ) -> std::io::Result<Self::FieldShare>;
    fn mul_with_public(&mut self, a: &F, b: &Self::FieldShare) -> Self::FieldShare;
    fn inv(&mut self, a: &Self::FieldShare) -> std::io::Result<Self::FieldShare>;
    fn neg(&mut self, a: &Self::FieldShare) -> Self::FieldShare;
    fn rand(&mut self) -> std::io::Result<Self::FieldShare>;
    fn open(&mut self, a: &Self::FieldShare) -> std::io::Result<F>;
    fn mul_vec(
        &mut self,
        a: &Self::FieldShareSlice<'_>,
        b: &Self::FieldShareSlice<'_>,
    ) -> std::io::Result<Self::FieldShareVec>;
    fn promote_to_trivial_share(&self, public_values: &[F]) -> Self::FieldShareVec;
    fn distribute_powers_and_mul_by_const(
        &mut self,
        coeffs: &mut Self::FieldShareSliceMut<'_>,
        g: F,
        c: F,
    );
    fn evaluate_constraint(
        &mut self,
        lhs: &[(F, usize)],
        public_inputs: &[F],
        private_witness: &Self::FieldShareSlice<'_>,
    ) -> Self::FieldShare;
    fn clone_from_slice(
        &self,
        dst: &mut Self::FieldShareSliceMut<'_>,
        src: &Self::FieldShareSlice<'_>,
        dst_offset: usize,
        src_offset: usize,
        len: usize,
    );

    fn print(&self, to_print: &Self::FieldShareVec);
    fn print_slice(&self, to_print: &Self::FieldShareSlice<'_>);
}

pub trait EcMpcProtocol<C: CurveGroup>: PrimeFieldMpcProtocol<C::ScalarField> {
    type PointShare;
    fn add_points(&mut self, a: &Self::PointShare, b: &Self::PointShare) -> Self::PointShare;
    fn sub_points(&mut self, a: &Self::PointShare, b: &Self::PointShare) -> Self::PointShare;
    fn add_assign_points(&mut self, a: &mut Self::PointShare, b: &Self::PointShare);
    fn sub_assign_points(&mut self, a: &mut Self::PointShare, b: &Self::PointShare);
    fn add_assign_points_public(&mut self, a: &mut Self::PointShare, b: &C);
    fn sub_assign_points_public(&mut self, a: &mut Self::PointShare, b: &C);
    fn add_assign_points_public_affine(&mut self, a: &mut Self::PointShare, b: &C::Affine);
    fn sub_assign_points_public_affine(&mut self, a: &mut Self::PointShare, b: &C::Affine);
    fn scalar_mul_public_point(&mut self, a: &C, b: &Self::FieldShare) -> Self::PointShare;
    fn scalar_mul_public_scalar(
        &mut self,
        a: &Self::PointShare,
        b: &C::ScalarField,
    ) -> Self::PointShare;
    fn scalar_mul(
        &mut self,
        a: &Self::PointShare,
        b: &Self::FieldShare,
    ) -> std::io::Result<Self::PointShare>;
    fn open_point(&mut self, a: &Self::PointShare) -> std::io::Result<C>;
}

pub trait PairingEcMpcProtocol<P: Pairing>: EcMpcProtocol<P::G1> + EcMpcProtocol<P::G2> {
    fn open_two_points(
        &mut self,
        a: &<Self as EcMpcProtocol<P::G1>>::PointShare,
        b: &<Self as EcMpcProtocol<P::G2>>::PointShare,
    ) -> std::io::Result<(P::G1, P::G2)>;
}

pub trait FFTProvider<F: PrimeField>: PrimeFieldMpcProtocol<F> {
    fn fft<D: EvaluationDomain<F>>(
        &mut self,
        data: Self::FieldShareSlice<'_>,
        domain: &D,
    ) -> Self::FieldShareVec;
    fn fft_in_place<D: EvaluationDomain<F>>(
        &mut self,
        data: &mut Self::FieldShareSliceMut<'_>,
        domain: &D,
    );
    fn ifft<D: EvaluationDomain<F>>(
        &mut self,
        data: &Self::FieldShareSlice<'_>,
        domain: &D,
    ) -> Self::FieldShareVec;
    fn ifft_in_place<D: EvaluationDomain<F>>(
        &mut self,
        data: &mut Self::FieldShareSliceMut<'_>,
        domain: &D,
    );
}

pub trait MSMProvider<C: CurveGroup>: EcMpcProtocol<C> {
    fn msm_public_points(
        &mut self,
        points: &[C::Affine],
        scalars: Self::FieldShareSlice<'_>,
    ) -> Self::PointShare;
}