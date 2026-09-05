// @ohhive/schema — hand-maintained TS mirror of the JSON Schemas in this package.
// If you change a .schema.json, change the type here in the same commit.

export type Modality = "text" | "code" | "image" | "video" | "speech" | "music";
export type ToolsLevel = "inference_only" | "sandboxed_tools";

export interface License {
  kind: "owner_only" | "open_source";
  spdx?: string;
}

export interface CardCapabilities {
  model_id?: string;
  min_vram_gb?: number;
  min_ram_gb?: number;
  tools_level?: ToolsLevel;
}

export interface PlanCard {
  key: string;
  title: string;
  modality: Modality;
  inputs: string;
  deps?: string[];
  acceptance: string;
  requires_internet?: boolean;
  required_capabilities?: CardCapabilities;
}

/** Output of the interviewer agent. See project-plan.schema.json (ADR-006 D37). */
export interface ProjectPlan {
  schema_version: 1;
  title: string;
  goal: string;
  license: License;
  requires_internet: boolean;
  cards: PlanCard[];
}

export const PROJECT_PLAN_SCHEMA_VERSION = 1 as const;

/** Cheap structural check; full validation happens server-side with the JSON Schema. */
export function isProjectPlan(x: unknown): x is ProjectPlan {
  if (typeof x !== "object" || x === null) return false;
  const p = x as Record<string, unknown>;
  return (
    p.schema_version === 1 &&
    typeof p.title === "string" &&
    typeof p.goal === "string" &&
    typeof p.requires_internet === "boolean" &&
    Array.isArray(p.cards) &&
    p.cards.length > 0
  );
}
