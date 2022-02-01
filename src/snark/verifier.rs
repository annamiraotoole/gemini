//! The verifier for the algebraic proofs.
use ark_ec::PairingEngine;
use ark_ff::Field;

use crate::circuit::R1cs;
use crate::kzg::VerifierKey;
use crate::misc::{evaluate_le, ip};
use crate::misc::{hadamard, powers, product_matrix_vector, tensor};
use crate::snark::Proof;
use crate::sumcheck::Subclaim;
use crate::transcript::GeminiTranscript;
use crate::{VerificationError, VerificationResult, PROTOCOL_NAME};

impl<E: PairingEngine> Proof<E> {
    /// Verification function for SNARK proof.
    /// The input contains the R1CS instance and the verification key
    /// of polynomial commitment.
    pub fn verify(&self, r1cs: &R1cs<E::Fr>, vk: &VerifierKey<E>) -> VerificationResult {
        let mut transcript = merlin::Transcript::new(PROTOCOL_NAME);
        let witness_commitment = self.witness_commitment;

        transcript.append_commitment(b"witness", &witness_commitment);
        let alpha: E::Fr = transcript.get_challenge(b"alpha");
        let first_sumcheck_msgs = &self.first_sumcheck_msgs;

        // Verify the first sumcheck
        transcript.append_scalar(b"zc(alpha)", &self.zc_alpha);

        let subclaim_1 = Subclaim::new(&mut transcript, first_sumcheck_msgs, self.zc_alpha)?;

        let eta = transcript.get_challenge::<E::Fr>(b"eta");
        let eta2 = eta.square();

        let num_constraints = r1cs.a.len();
        let tensor_challenges = tensor(&subclaim_1.challenges);
        let tensor_challenges_head = &tensor_challenges[..num_constraints];
        let alpha_powers = powers(alpha, num_constraints);
        let hadamard_randomness = hadamard(tensor_challenges_head, &alpha_powers);

        // Verify the second sumcheck
        let asserted_sum_2 = subclaim_1.final_foldings[0][0]
            + subclaim_1.final_foldings[0][1] * eta
            + self.zc_alpha * eta2;

        let subclaim_2 =
            Subclaim::new(&mut transcript, &self.second_sumcheck_msgs, asserted_sum_2)?;

        // Consistency check
        let gamma = transcript.get_challenge::<E::Fr>(b"batch_challenge");
        self.tensorcheck_proof
            .folded_polynomials_commitments
            .iter()
            .for_each(|c| transcript.append_commitment(b"commitment", c));
        let beta = transcript.get_challenge::<E::Fr>(b"evaluation-chal");
        let beta_powers = powers(beta, num_constraints);
        let minus_beta_powers = powers(-beta, num_constraints);

        let m_pos = ip(
            &product_matrix_vector(&r1cs.a, &beta_powers),
            &hadamard_randomness,
        ) + eta
            * ip(
                &product_matrix_vector(&r1cs.b, &beta_powers),
                tensor_challenges_head,
            )
            + eta2 * ip(&product_matrix_vector(&r1cs.c, &beta_powers), &alpha_powers);
        let m_neg = ip(
            &product_matrix_vector(&r1cs.a, &minus_beta_powers),
            &hadamard_randomness,
        ) + eta
            * ip(
                &product_matrix_vector(&r1cs.b, &minus_beta_powers),
                tensor_challenges_head,
            )
            + eta2
                * ip(
                    &product_matrix_vector(&r1cs.c, &minus_beta_powers),
                    &alpha_powers,
                );

        let beta_power = E::Fr::pow(&beta, &[r1cs.x.len() as u64]);
        let z_pos = evaluate_le(&r1cs.x, &beta)
            + beta_power * self.tensorcheck_proof.base_polynomials_evaluations[0][1];
        let z_neg = if (r1cs.x.len() & 1) == 0 {
            evaluate_le(&r1cs.x, &-beta)
                + beta_power * self.tensorcheck_proof.base_polynomials_evaluations[0][2]
        } else {
            evaluate_le(&r1cs.x, &-beta)
                - beta_power * self.tensorcheck_proof.base_polynomials_evaluations[0][2]
        };

        let direct_base_polynomials_evaluations =
            vec![[m_pos + gamma * z_pos, m_neg + gamma * z_neg]];

        self.tensorcheck_proof
            .verify(
                &mut transcript,
                vk,
                &[subclaim_2.final_foldings[0].to_vec()],
                &[self.witness_commitment],
                &direct_base_polynomials_evaluations,
                &[subclaim_2.challenges],
                beta,
                gamma,
            )
            .map_err(|_| VerificationError)
    }
}
