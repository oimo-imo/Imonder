import { EditableMesh } from "../core/EditableMesh";

export type ModifierType = "bevel";

export interface Modifier {
  id: string;
  type: ModifierType;
  enabled: boolean;
  name: string;
}

export interface ModifierEvaluator {
  apply(mesh: EditableMesh, modifier: Modifier): EditableMesh;
}

/** Runs the enabled modifiers of a stack in order over a base mesh. */
export function evaluateStack(
  base: EditableMesh,
  stack: Modifier[],
  evaluators: Record<ModifierType, ModifierEvaluator>,
): EditableMesh {
  let result = base;
  for (const modifier of stack) {
    if (!modifier.enabled) continue;
    const evaluator = evaluators[modifier.type];
    result = evaluator.apply(result, modifier);
  }
  return result;
}
