use std::marker::PhantomData;

use ark_ec::pairing::Pairing;
use ark_ec::{AffineRepr, CurveGroup};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof, ProvingKey};
use ark_poly::{EvaluationDomain, GeneralEvaluationDomain};
use ark_relations::r1cs::Result as R1CSResult;
use ark_relations::r1cs::{
    ConstraintMatrices, ConstraintSystem, ConstraintSystemRef, LinearCombination, OptimizationGoal,
    SynthesisError, Variable,
};
use circom_types::r1cs::R1CS;
use color_eyre::eyre::Result;
use itertools::izip;
use mpc_core::protocols::aby3::network::Aby3Network;
use mpc_core::protocols::gsz::network::GSZNetwork;
use mpc_core::protocols::gsz::GSZProtocol;
use mpc_core::protocols::{aby3, gsz};
use mpc_core::traits::{EcMpcProtocol, MSMProvider};
use mpc_core::{
    protocols::aby3::{network::Aby3MpcNet, Aby3Protocol},
    traits::{FFTProvider, PairingEcMpcProtocol, PrimeFieldMpcProtocol},
};
use mpc_net::config::NetworkConfig;
use num_traits::identities::One;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};

pub type Aby3CollaborativeGroth16<P> =
    CollaborativeGroth16<Aby3Protocol<<P as Pairing>::ScalarField, Aby3MpcNet>, P>;

type FieldShare<T, P> = <T as PrimeFieldMpcProtocol<<P as Pairing>::ScalarField>>::FieldShare;
type FieldShareVec<T, P> = <T as PrimeFieldMpcProtocol<<P as Pairing>::ScalarField>>::FieldShareVec;
type ScalarFieldShareSlice<'a, T, P> =
    <T as PrimeFieldMpcProtocol<<P as Pairing>::ScalarField>>::FieldShareSlice<'a>;
type FieldShareSliceMut<'a, T, P> =
    <T as PrimeFieldMpcProtocol<<P as Pairing>::ScalarField>>::FieldShareSliceMut<'a>;
type PointShare<T, C> = <T as EcMpcProtocol<C>>::PointShare;
type CurveFieldShareSlice<'a, T, C> = <T as PrimeFieldMpcProtocol<
    <<C as CurveGroup>::Affine as AffineRepr>::ScalarField,
>>::FieldShareSlice<'a>;

//FIXME I want to use serde(transparent) but not working
#[derive(Serialize, Deserialize)]
pub struct SharedWitness<T, P: Pairing>
where
    T: PrimeFieldMpcProtocol<P::ScalarField>,
{
    pub values: FieldShareVec<T, P>,
}

pub struct CollaborativeGroth16<T, P: Pairing>
where
    for<'a> T: PrimeFieldMpcProtocol<P::ScalarField>
        + PairingEcMpcProtocol<P>
        + FFTProvider<P::ScalarField>
        + MSMProvider<P::G1>
        + MSMProvider<P::G2>,
{
    pub(crate) driver: T,
    phantom_data: PhantomData<P>,
}

impl<T, P: Pairing> CollaborativeGroth16<T, P>
where
    for<'a> T: PrimeFieldMpcProtocol<P::ScalarField>
        + PairingEcMpcProtocol<P>
        + FFTProvider<P::ScalarField>
        + MSMProvider<P::G1>
        + MSMProvider<P::G2>,
{
    pub fn new(driver: T) -> Self {
        Self {
            driver,
            phantom_data: PhantomData,
        }
    }
    pub fn prove(
        &mut self,
        pk: &ProvingKey<P>,
        r1cs: &R1CS<P>,
        public_inputs: &[P::ScalarField],
        private_witness: SharedWitness<T, P>,
    ) -> Result<Proof<P>> {
        let cs = ConstraintSystem::new_ref();
        cs.set_optimization_goal(OptimizationGoal::Constraints);
        Self::generate_constraints(public_inputs, r1cs, cs.clone())?;
        let matrices = cs.to_matrices().unwrap();
        let num_inputs = cs.num_instance_variables();
        let num_constraints = cs.num_constraints();
        let private_witness = private_witness.get_ref();
        let h = self.witness_map_from_matrices(
            &matrices,
            num_constraints,
            num_inputs,
            public_inputs,
            private_witness,
        )?;
        let h_slice = ScalarFieldShareSlice::<T, P>::from(&h);
        let r = self.driver.rand()?;
        let s = self.driver.rand()?;
        self.create_proof_with_assignment(pk, r, s, h_slice, &public_inputs[1..], private_witness)
    }

    fn witness_map_from_matrices(
        &mut self,
        matrices: &ConstraintMatrices<P::ScalarField>,
        num_constraints: usize,
        num_inputs: usize,
        public_inputs: &[P::ScalarField],
        private_witness: ScalarFieldShareSlice<T, P>,
    ) -> Result<FieldShareVec<T, P>> {
        let domain = GeneralEvaluationDomain::<P::ScalarField>::new(num_constraints + num_inputs)
            .ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
        let domain_size = domain.size();
        let mut a = vec![FieldShare::<T, P>::default(); domain_size];
        let mut b = vec![FieldShare::<T, P>::default(); domain_size];
        for (a, b, at_i, bt_i) in izip!(&mut a, &mut b, &matrices.a, &matrices.b) {
            *a = self
                .driver
                .evaluate_constraint(at_i, public_inputs, &private_witness);
            *b = self
                .driver
                .evaluate_constraint(bt_i, public_inputs, &private_witness);
        }
        let mut a = FieldShareVec::<T, P>::from(a);
        {
            let mut a_mut = FieldShareSliceMut::<T, P>::from(&mut a);
            let promoted_public = self.driver.promote_to_trivial_share(public_inputs);
            self.driver.clone_from_slice(
                &mut a_mut,
                &ScalarFieldShareSlice::<T, P>::from(&promoted_public),
                num_constraints,
                0,
                num_inputs,
            );
        }

        let mut b = FieldShareVec::<T, P>::from(b);
        let mut c = {
            let a_slice = ScalarFieldShareSlice::<T, P>::from(&a);
            let b_slice = ScalarFieldShareSlice::<T, P>::from(&b);
            self.driver.mul_vec(&a_slice, &b_slice)?
        };

        let mut a_mut = FieldShareSliceMut::<T, P>::from(&mut a);
        let mut b_mut = FieldShareSliceMut::<T, P>::from(&mut b);

        self.driver.ifft_in_place(&mut a_mut, &domain);
        self.driver.ifft_in_place(&mut b_mut, &domain);
        let root_of_unity = {
            let domain_size_double = 2 * domain_size;
            let domain_double = GeneralEvaluationDomain::new(domain_size_double)
                .ok_or(SynthesisError::PolynomialDegreeTooLarge)?;
            domain_double.element(1)
        };
        self.driver.distribute_powers_and_mul_by_const(
            &mut a_mut,
            root_of_unity,
            P::ScalarField::one(),
        );
        self.driver.distribute_powers_and_mul_by_const(
            &mut b_mut,
            root_of_unity,
            P::ScalarField::one(),
        );
        self.driver.fft_in_place(&mut a_mut, &domain);
        self.driver.fft_in_place(&mut b_mut, &domain);
        std::mem::drop(a_mut);
        std::mem::drop(b_mut);
        let mut ab = {
            let a_slice = ScalarFieldShareSlice::<T, P>::from(&a);
            let b_slice = ScalarFieldShareSlice::<T, P>::from(&b);
            //this can be in-place so that we do not have to allocate memory
            self.driver.mul_vec(&a_slice, &b_slice)?
        };
        std::mem::drop(a);
        std::mem::drop(b);

        let mut c_mut = FieldShareSliceMut::<T, P>::from(&mut c);
        self.driver.ifft_in_place(&mut c_mut, &domain);
        self.driver.distribute_powers_and_mul_by_const(
            &mut c_mut,
            root_of_unity,
            P::ScalarField::one(),
        );
        self.driver.fft_in_place(&mut c_mut, &domain);
        std::mem::drop(c_mut);

        let mut ab_mut = FieldShareSliceMut::<T, P>::from(&mut ab);
        let c_slice = ScalarFieldShareSlice::<T, P>::from(&c);
        self.driver.sub_assign_vec(&mut ab_mut, &c_slice);
        std::mem::drop(ab_mut);
        Ok(ab)
    }

    fn generate_constraints(
        public_inputs: &[P::ScalarField],
        r1cs: &R1CS<P>,
        cs: ConstraintSystemRef<P::ScalarField>,
    ) -> Result<()> {
        for f in public_inputs.iter().skip(1) {
            cs.new_input_variable(|| Ok(*f))?;
        }

        let make_index = |index| {
            if index < r1cs.num_inputs {
                Variable::Instance(index)
            } else {
                Variable::Witness(index - r1cs.num_inputs)
            }
        };
        let make_lc = |lc_data: &[(usize, P::ScalarField)]| {
            lc_data.iter().fold(
                LinearCombination::<P::ScalarField>::zero(),
                |lc: LinearCombination<P::ScalarField>, (index, coeff)| {
                    lc + (*coeff, make_index(*index))
                },
            )
        };

        for constraint in &r1cs.constraints {
            cs.enforce_constraint(
                make_lc(&constraint.0),
                make_lc(&constraint.1),
                make_lc(&constraint.2),
            )?;
        }
        cs.finalize();
        Ok(())
    }

    fn calculate_coeff<C: CurveGroup>(
        &mut self,
        initial: PointShare<T, C>,
        query: &[C::Affine],
        vk_param: C::Affine,
        input_assignment: &[C::ScalarField],
        aux_assignment: CurveFieldShareSlice<'_, T, C>,
    ) -> PointShare<T, C>
    where
        T: EcMpcProtocol<C>,
        T: MSMProvider<C>,
    {
        let pub_len = input_assignment.len();
        let pub_acc = C::msm_unchecked(&query[1..=pub_len], input_assignment);
        let priv_acc = MSMProvider::<C>::msm_public_points(
            &mut self.driver,
            &query[1 + pub_len..],
            aux_assignment,
        );

        let mut res = initial;
        EcMpcProtocol::<C>::add_assign_points_public_affine(&mut self.driver, &mut res, &query[0]);
        EcMpcProtocol::<C>::add_assign_points_public_affine(&mut self.driver, &mut res, &vk_param);
        EcMpcProtocol::<C>::add_assign_points_public(&mut self.driver, &mut res, &pub_acc);
        EcMpcProtocol::<C>::add_assign_points(&mut self.driver, &mut res, &priv_acc);

        res
    }

    pub fn create_proof_with_assignment(
        &mut self,
        pk: &ProvingKey<P>,
        r: FieldShare<T, P>,
        s: FieldShare<T, P>,
        h: ScalarFieldShareSlice<'_, T, P>,
        input_assignment: &[P::ScalarField],
        aux_assignment: ScalarFieldShareSlice<'_, T, P>,
    ) -> Result<Proof<P>> {
        //let c_acc_time = start_timer!(|| "Compute C");
        let h_acc = MSMProvider::<P::G1>::msm_public_points(&mut self.driver, &pk.h_query, h);

        // Compute C
        let l_aux_acc =
            MSMProvider::<P::G1>::msm_public_points(&mut self.driver, &pk.l_query, aux_assignment);

        let delta_g1 = pk.delta_g1.into_group();
        let rs = self.driver.mul(&r, &s)?;
        let r_s_delta_g1 = self.driver.scalar_mul_public_point(&delta_g1, &rs);

        //end_timer!(c_acc_time);

        // Compute A
        // let a_acc_time = start_timer!(|| "Compute A");
        let r_g1 = self.driver.scalar_mul_public_point(&delta_g1, &r);

        let g_a = self.calculate_coeff::<P::G1>(
            r_g1,
            &pk.a_query,
            pk.vk.alpha_g1,
            input_assignment,
            aux_assignment,
        );

        // Open here since g_a is part of proof
        let g_a_opened = EcMpcProtocol::<P::G1>::open_point(&mut self.driver, &g_a)?;
        let s_g_a = self.driver.scalar_mul_public_point(&g_a_opened, &s);
        // end_timer!(a_acc_time);

        // Compute B in G1
        // In original implementation this is skipped if r==0, however r is shared in our case
        //  let b_g1_acc_time = start_timer!(|| "Compute B in G1");
        let s_g1 = self.driver.scalar_mul_public_point(&delta_g1, &s);
        let g1_b = self.calculate_coeff::<P::G1>(
            s_g1,
            &pk.b_g1_query,
            pk.beta_g1,
            input_assignment,
            aux_assignment,
        );
        let r_g1_b = EcMpcProtocol::<P::G1>::scalar_mul(&mut self.driver, &g1_b, &r)?;
        //  end_timer!(b_g1_acc_time);

        // Compute B in G2
        let delta_g2 = pk.vk.delta_g2.into_group();
        //  let b_g2_acc_time = start_timer!(|| "Compute B in G2");
        let s_g2 = self.driver.scalar_mul_public_point(&delta_g2, &s);
        let g2_b = self.calculate_coeff::<P::G2>(
            s_g2,
            &pk.b_g2_query,
            pk.vk.beta_g2,
            input_assignment,
            aux_assignment,
        );
        // end_timer!(b_g2_acc_time);

        //  let c_time = start_timer!(|| "Finish C");
        let mut g_c = s_g_a;
        EcMpcProtocol::<P::G1>::add_assign_points(&mut self.driver, &mut g_c, &r_g1_b);
        EcMpcProtocol::<P::G1>::sub_assign_points(&mut self.driver, &mut g_c, &r_s_delta_g1);
        EcMpcProtocol::<P::G1>::add_assign_points(&mut self.driver, &mut g_c, &l_aux_acc);
        EcMpcProtocol::<P::G1>::add_assign_points(&mut self.driver, &mut g_c, &h_acc);
        //  end_timer!(c_time);

        let (g_c_opened, g2_b_opened) =
            PairingEcMpcProtocol::<P>::open_two_points(&mut self.driver, &g_c, &g2_b)?;

        Ok(Proof {
            a: g_a_opened.into_affine(),
            b: g2_b_opened.into_affine(),
            c: g_c_opened.into_affine(),
        })
    }

    pub fn verify(
        &self,
        pvk: &PreparedVerifyingKey<P>,
        proof: &Proof<P>,
        public_inputs: &[P::ScalarField],
    ) -> R1CSResult<bool> {
        Groth16::<P>::verify_proof(pvk, proof, public_inputs)
    }
}

impl<P: Pairing> Aby3CollaborativeGroth16<P> {
    pub fn with_network_config(config: NetworkConfig) -> Result<Self> {
        let mpc_net = Aby3MpcNet::new(config)?;
        let driver = Aby3Protocol::<P::ScalarField, Aby3MpcNet>::new(mpc_net)?;
        Ok(CollaborativeGroth16::new(driver))
    }
}

impl<T, P: Pairing> SharedWitness<T, P>
where
    T: PrimeFieldMpcProtocol<P::ScalarField>,
{
    fn get_ref(&self) -> T::FieldShareSlice<'_> {
        T::FieldShareSlice::from(&self.values)
    }
}

impl<N: Aby3Network, P: Pairing> SharedWitness<Aby3Protocol<P::ScalarField, N>, P> {
    pub fn share_aby3<R: Rng + CryptoRng>(witness: Vec<P::ScalarField>, rng: &mut R) -> [Self; 3] {
        let [share1, share2, share3] = aby3::utils::share_field_elements(witness, rng);
        let witness1 = Self { values: share1 };
        let witness2 = Self { values: share2 };
        let witness3 = Self { values: share3 };
        [witness1, witness2, witness3]
    }
}

impl<N: GSZNetwork, P: Pairing> SharedWitness<GSZProtocol<P::ScalarField, N>, P> {
    pub fn share_gsz<R: Rng + CryptoRng>(
        witness: Vec<P::ScalarField>,
        degree: usize,
        num_parties: usize,
        rng: &mut R,
    ) -> Vec<Self> {
        let shares = gsz::utils::share_field_elements(witness, degree, num_parties, rng);

        shares
            .into_iter()
            .map(|share| Self { values: share })
            .collect()
    }
}

#[cfg(test)]
mod test {
    use std::fs::File;

    use ark_bn254::Bn254;
    use circom_types::groth16::witness::Witness;
    use mpc_core::protocols::{
        aby3::{network::Aby3MpcNet, Aby3Protocol},
        gsz::{network::GSZMpcNet, GSZProtocol},
    };
    use rand::thread_rng;

    use super::SharedWitness;

    #[ignore]
    #[test]
    fn test_aby3() {
        let witness_file = File::open("../test_vectors/bn254/multiplier2/witness.wtns").unwrap();
        let witness = Witness::<ark_bn254::Fr>::from_reader(witness_file).unwrap();
        let mut rng = thread_rng();
        let [s1, _, _] =
            SharedWitness::<Aby3Protocol<ark_bn254::Fr, Aby3MpcNet>, Bn254>::share_aby3(
                witness.values,
                &mut rng,
            );
        println!("{}", serde_json::to_string(&s1.values).unwrap());
    }

    fn test_gsz_inner(num_parties: usize, threshold: usize) {
        let witness_file = File::open("../test_vectors/bn254/multiplier2/witness.wtns").unwrap();
        let witness = Witness::<ark_bn254::Fr>::from_reader(witness_file).unwrap();
        let mut rng = thread_rng();
        let s1 = SharedWitness::<GSZProtocol<ark_bn254::Fr, GSZMpcNet>, Bn254>::share_gsz(
            witness.values,
            threshold,
            num_parties,
            &mut rng,
        );
        println!("{}", serde_json::to_string(&s1[0].values).unwrap());
    }

    #[ignore]
    #[test]
    fn test_gsz() {
        test_gsz_inner(3, 1);
    }
}