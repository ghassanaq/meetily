export interface ExpertProfileSummary {
  id: string;
  name: string;
  retired_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface StoredProfileVersion {
  profile_id: string;
  version_hash: string;
  seq: number;
  schema_version: number;
  created_at: string;
}

export interface StoredEvalPlan {
  id: string;
  profile_id: string;
  content_hash: string;
  schema_version: number;
  created_at: string;
}

export interface ModelGenerationBinding {
  provider: string;
  model: string;
  prompt_renderer_hash: string;
  output_parser_version: number;
}

export interface ProfileActivationView {
  activation: {
    profile_id: string;
    profile_version_hash: string;
    capability_revision_hash: string;
    eval_run_id: number;
    status: 'active' | 'superseded';
    superseded_reason: string | null;
    activated_at: string;
  };
  binding: ModelGenerationBinding;
}

export interface SemanticAssertionResult {
  assertion_index: number;
  adjudicator: 'human' | 'model';
  threshold: number;
  score: number | null;
  passed: boolean | null;
}

export interface EvalRepetitionResult {
  target: 'candidate' | 'baseline';
  case_id: string;
  playbook_id: string;
  repetition: number;
  hard: Array<{ assertion: string; passed: boolean; detail: string }>;
  semantic: SemanticAssertionResult[];
  output_markdown: string | null;
  generation_error: string | null;
}

export interface EvaluationReport {
  qualifying: boolean;
  candidate_profile_version_hash: string;
  baseline_profile_version_hash: string | null;
  candidate_capability_hash: string;
  eval_plan_hash: string;
  model_binding_hash: string;
  safety_gate_version: string;
  repetitions: EvalRepetitionResult[];
  baseline_missing_playbooks: string[];
  removed_playbooks: string[];
  outcome: 'pass' | 'fail' | 'rejected' | 'inconclusive' | 'baseline_missing';
  reasons: string[];
}

export interface ProfileEvalResponse {
  run: { id: number; outcome: string };
  report: EvaluationReport;
}
