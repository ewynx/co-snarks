//! This crate defines the plain [Plonk] type. Plain in this context means without MPC. For the
//! co-PLONK prover, see [CoPlonk].
//!
//! You will most likely need the plain PLONK implementation to verify a proof from co-PLONK. For that
//! see the [`Plonk::verify`] method.

use crate::{plonk_utils, types::Domains, CoPlonk};
use ark_ec::{pairing::Pairing, Group};
use ark_ff::Field;
use circom_types::{
    plonk::{JsonVerificationKey, PlonkProof},
    traits::{CircomArkworksPairingBridge, CircomArkworksPrimeFieldBridge},
};
use mpc_core::{protocols::plain::PlainDriver, traits::FFTPostProcessing};
use num_traits::One;
use num_traits::Zero;

use crate::types::Keccak256Transcript;

/// The plain [`Plonk`] type.
///
/// This type is actually the [`CoPlonk`] type initialized with
/// the [`PlainDriver`], a single party (you) MPC protocol (i.e., your everyday PLONK).
/// You can use this instance to create a proof, but we recommend against it for a real use-case.
/// The co-PLONK prover uses some MPC optimizations (for the product check), which are not optimal
/// for a plain run.
///
/// More interesting is the [`Plonk::verify`] method. You can verify any circom PLONK proof, be it
/// from snarkjs or one created by this project.
pub type Plonk<P> = CoPlonk<PlainDriver<<P as Pairing>::ScalarField>, P>;

pub(crate) struct VerifierChallenges<P: Pairing> {
    pub(super) alpha: P::ScalarField,
    pub(super) beta: P::ScalarField,
    pub(super) gamma: P::ScalarField,
    pub(super) xi: P::ScalarField,
    pub(super) v: [P::ScalarField; 5],
    pub(super) u: P::ScalarField,
}

impl<P: Pairing> VerifierChallenges<P>
where
    P: CircomArkworksPairingBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
    P::ScalarField: CircomArkworksPrimeFieldBridge,
{
    pub(super) fn new(
        vk: &JsonVerificationKey<P>,
        proof: &PlonkProof<P>,
        public_inputs: &[P::ScalarField],
    ) -> Self {
        let mut transcript = Keccak256Transcript::<P>::default();

        // Challenge round 2: beta and gamma
        transcript.add_point(vk.qm);
        transcript.add_point(vk.ql);
        transcript.add_point(vk.qr);
        transcript.add_point(vk.qo);
        transcript.add_point(vk.qc);
        transcript.add_point(vk.s1);
        transcript.add_point(vk.s2);
        transcript.add_point(vk.s3);

        for p in public_inputs.iter().cloned() {
            transcript.add_scalar(p);
        }

        transcript.add_point(proof.a);
        transcript.add_point(proof.b);
        transcript.add_point(proof.c);

        let beta = transcript.get_challenge();

        let mut transcript = Keccak256Transcript::<P>::default();
        transcript.add_scalar(beta);
        let gamma = transcript.get_challenge();

        // Challenge round 3: alpha
        let mut transcript = Keccak256Transcript::<P>::default();
        transcript.add_scalar(beta);
        transcript.add_scalar(gamma);
        transcript.add_point(proof.z);
        let alpha = transcript.get_challenge();

        // Challenge round 4: xi
        let mut transcript = Keccak256Transcript::<P>::default();
        transcript.add_scalar(alpha);
        transcript.add_point(proof.t1);
        transcript.add_point(proof.t2);
        transcript.add_point(proof.t3);
        let xi = transcript.get_challenge();

        // Challenge round 5: v
        let mut transcript = Keccak256Transcript::<P>::default();
        transcript.add_scalar(xi);
        transcript.add_scalar(proof.eval_a);
        transcript.add_scalar(proof.eval_b);
        transcript.add_scalar(proof.eval_c);
        transcript.add_scalar(proof.eval_s1);
        transcript.add_scalar(proof.eval_s2);
        transcript.add_scalar(proof.eval_zw);
        let mut v = [P::ScalarField::zero(); 5];
        v[0] = transcript.get_challenge();

        for i in 1..5 {
            v[i] = v[i - 1] * v[0];
        }

        // Challenge: u
        let mut transcript = Keccak256Transcript::<P>::default();
        transcript.add_point(proof.wxi);
        transcript.add_point(proof.wxiw);
        let u = transcript.get_challenge();
        Self {
            alpha,
            beta,
            gamma,
            xi,
            v,
            u,
        }
    }
}

impl<P: Pairing> Plonk<P>
where
    P::ScalarField: CircomArkworksPrimeFieldBridge,
    P: Pairing + CircomArkworksPairingBridge,
    P::BaseField: CircomArkworksPrimeFieldBridge,
    P::ScalarField: FFTPostProcessing,
{
    /// Verifies a circom PLONK proof. The method uses the same interface as snarkjs and it can verify
    /// proofs generated by snarkjs and by this project.
    pub fn verify(
        vk: &JsonVerificationKey<P>,
        proof: &PlonkProof<P>,
        public_inputs: &[P::ScalarField],
    ) -> Result<bool, eyre::Report>
    where
        P: Pairing,
        P: CircomArkworksPairingBridge,
        P::BaseField: CircomArkworksPrimeFieldBridge,
        P::ScalarField: CircomArkworksPrimeFieldBridge,
    {
        if vk.n_public != public_inputs.len() {
            return Err(eyre::eyre!("Invalid number of public inputs"));
        }

        let challenges = VerifierChallenges::<P>::new(vk, proof, public_inputs);
        let domains = Domains::<P::ScalarField>::new(1 << vk.power)?;

        let (l, xin) = plonk_utils::calculate_lagrange_evaluations::<P>(
            vk.power,
            vk.n_public,
            &challenges.xi,
            &domains,
        );
        let pi = plonk_utils::calculate_pi::<P>(public_inputs, &l);
        let (r0, d) = Plonk::<P>::calculate_r0_d(vk, proof, &challenges, pi, &l[0], xin);

        let e = Plonk::<P>::calculate_e(proof, &challenges, r0);
        let f = Plonk::<P>::calculate_f(vk, proof, &challenges, d);

        Ok(Plonk::<P>::valid_pairing(
            vk,
            proof,
            &challenges,
            e,
            f,
            &domains,
        ))
    }

    pub(crate) fn calculate_r0_d(
        vk: &JsonVerificationKey<P>,
        proof: &PlonkProof<P>,
        challenges: &VerifierChallenges<P>,
        pi: P::ScalarField,
        l0: &P::ScalarField,
        xin: P::ScalarField,
    ) -> (P::ScalarField, P::G1)
    where
        P: CircomArkworksPairingBridge,
        P::BaseField: CircomArkworksPrimeFieldBridge,
        P::ScalarField: CircomArkworksPrimeFieldBridge,
    {
        // R0
        let e1 = pi;
        let e2 = challenges.alpha.square() * l0;
        let e3a = proof.eval_a + proof.eval_s1 * challenges.beta + challenges.gamma;
        let e3b = proof.eval_b + proof.eval_s2 * challenges.beta + challenges.gamma;
        let e3c = proof.eval_c + challenges.gamma;

        let e3 = e3a * e3b * e3c * proof.eval_zw * challenges.alpha;
        let r0 = e1 - e2 - e3;

        // D
        let d1 = vk.qm * (proof.eval_a * proof.eval_b)
            + vk.ql * proof.eval_a
            + vk.qr * proof.eval_b
            + vk.qo * proof.eval_c
            + vk.qc;

        let betaxi = challenges.beta * challenges.xi;
        let d2a1 = proof.eval_a + betaxi + challenges.gamma;
        let d2a2 = proof.eval_b + betaxi * vk.k1 + challenges.gamma;
        let d2a3 = proof.eval_c + betaxi * vk.k2 + challenges.gamma;
        let d2a = d2a1 * d2a2 * d2a3 * challenges.alpha;
        let d2b = e2;
        let d2 = proof.z * (d2a + d2b + challenges.u);

        let d3a = e3a;
        let d3b = e3b;
        let d3c = challenges.alpha * challenges.beta * proof.eval_zw;
        let d3 = vk.s3 * (d3a * d3b * d3c);

        let d4_low = proof.t1;
        let d4_mid = proof.t2 * xin;
        let d4_high = proof.t3 * xin.square();
        let d4 = (d4_low + d4_mid + d4_high) * (xin - P::ScalarField::one());

        let d = d1 + d2 - d3 - d4;

        (r0, d)
    }

    fn calculate_e(
        proof: &PlonkProof<P>,
        challenges: &VerifierChallenges<P>,
        r0: P::ScalarField,
    ) -> P::G1 {
        let e = challenges.v[0] * proof.eval_a
            + challenges.v[1] * proof.eval_b
            + challenges.v[2] * proof.eval_c
            + challenges.v[3] * proof.eval_s1
            + challenges.v[4] * proof.eval_s2
            + challenges.u * proof.eval_zw
            - r0;
        P::G1::generator() * e
    }

    fn calculate_f(
        vk: &JsonVerificationKey<P>,
        proof: &PlonkProof<P>,
        challenges: &VerifierChallenges<P>,
        d: P::G1,
    ) -> P::G1 {
        d + proof.a * challenges.v[0]
            + proof.b * challenges.v[1]
            + proof.c * challenges.v[2]
            + vk.s1 * challenges.v[3]
            + vk.s2 * challenges.v[4]
    }

    fn valid_pairing(
        vk: &JsonVerificationKey<P>,
        proof: &PlonkProof<P>,
        challenges: &VerifierChallenges<P>,
        e: P::G1,
        f: P::G1,
        domains: &Domains<P::ScalarField>,
    ) -> bool {
        let s = challenges.u * challenges.xi * domains.root_of_unity_pow;

        let a1 = proof.wxi + proof.wxiw * challenges.u;
        let b1 = proof.wxi * challenges.xi + proof.wxiw * s - e + f;

        let lhs = P::pairing(a1, vk.x2);
        let rhs = P::pairing(b1, P::G2::generator());

        lhs == rhs
    }
}

#[cfg(test)]
pub mod tests {
    use std::fs::File;

    use ark_bn254::Bn254;
    use circom_types::groth16::JsonPublicInput;
    use circom_types::plonk::{JsonVerificationKey, PlonkProof};
    use itertools::Itertools;

    use super::{Plonk, VerifierChallenges};
    use std::str::FromStr;
    #[test]
    pub fn calculate_verifier_challenges() {
        let vk: JsonVerificationKey<Bn254> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/multiplierAdd2/verification_key.json").unwrap(),
        )
        .unwrap();
        let proof: PlonkProof<Bn254> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/multiplierAdd2/circom.proof").unwrap(),
        )
        .unwrap();
        let public_inputs: JsonPublicInput<ark_bn254::Fr> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/multiplierAdd2/public.json").unwrap(),
        )
        .unwrap();

        let challenges = VerifierChallenges::new(&vk, &proof, &public_inputs.values);
        assert_eq!(
            challenges.alpha,
            ark_bn254::Fr::from_str(
                "15671686582917654457673979066076453089160319530775888548203655564244609989010"
            )
            .unwrap()
        );
        assert_eq!(
            challenges.beta,
            ark_bn254::Fr::from_str(
                "3263596124809895505836166591524490347058299776658091330921253084650986734968"
            )
            .unwrap()
        );
        assert_eq!(
            challenges.gamma,
            ark_bn254::Fr::from_str(
                "5067097558220314492899237494212876670476725908978950335663392883732324945306"
            )
            .unwrap()
        );
        assert_eq!(
            challenges.xi,
            ark_bn254::Fr::from_str(
                "3112051444889417241969819747591049287576627682236445513762445924191561705445"
            )
            .unwrap()
        );
        assert_eq!(
            challenges.v.to_vec(),
            vec![
                "19611229682101317528275240009855809914391062754050500133834153708898530490155",
                "4748783812041143390637207905098578563169927926236635348130330468945662641652",
                "5199380753164799358017571952644889432804973357781520358621237112694681049584",
                "19012210024703196990299057573623189786858603305437590364879974525894752886774",
                "5385442044724577504962048101695566214714730673101096202581966373431715274981"
            ]
            .into_iter()
            .map(|s| ark_bn254::Fr::from_str(s).unwrap())
            .collect_vec()
        );
        assert_eq!(
            challenges.u,
            ark_bn254::Fr::from_str(
                "13376185761335708482939014433471747421911583656433513500358285265522128506177"
            )
            .unwrap()
        );
    }

    #[test]
    pub fn verify_multiplier2_from_circom() {
        let vk: JsonVerificationKey<Bn254> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/multiplierAdd2/verification_key.json").unwrap(),
        )
        .unwrap();
        let proof: PlonkProof<Bn254> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/multiplierAdd2/circom.proof").unwrap(),
        )
        .unwrap();
        let public_inputs: JsonPublicInput<ark_bn254::Fr> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/multiplierAdd2/public.json").unwrap(),
        )
        .unwrap();
        assert!(Plonk::verify(&vk, &proof, &public_inputs.values).unwrap());
    }

    #[test]
    pub fn verify_poseidon_from_circom() {
        let vk: JsonVerificationKey<Bn254> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/poseidon/verification_key.json").unwrap(),
        )
        .unwrap();
        let proof: PlonkProof<Bn254> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/poseidon/circom.proof").unwrap(),
        )
        .unwrap();
        let public_inputs: JsonPublicInput<ark_bn254::Fr> = serde_json::from_reader(
            File::open("../test_vectors/Plonk/bn254/poseidon/public.json").unwrap(),
        )
        .unwrap();
        assert!(Plonk::verify(&vk, &proof, &public_inputs.values).unwrap());
    }
}