import type { PlanResponse } from "../api/types";
import PlanCard from "./PlanCard";

interface PlanPanelProps {
  plan: PlanResponse | null;
  onPlanChange: (plan: PlanResponse) => void;
  onRunStart: () => void;
}

function PlanPanel({ plan, onPlanChange, onRunStart }: PlanPanelProps) {
  if (!plan) {
    return (
      <div className="empty-state">
        <h2>等待搜索计划</h2>
        <p>输入中文主题后，这里会显示可审查和可编辑的搜索方向。</p>
      </div>
    );
  }

  return (
    <div className="plan-stack">
      <div className="section-header">
        <div>
          <p className="eyebrow">SearchPlan</p>
          <h2>{plan.original_topic}</h2>
        </div>
        <span className="badge">{plan.llm_generated ? "LLM" : "规则"}</span>
      </div>

      {plan.warnings.length > 0 && (
        <div className="warning-box">
          {plan.warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
        </div>
      )}

      <div className="aspect-list">
        {plan.aspects.map((aspect, index) => (
          <PlanCard
            key={`${plan.plan_id}-${index}`}
            aspect={aspect}
            index={index}
            onChange={(nextAspect) => {
              const nextAspects = [...plan.aspects];
              nextAspects[index] = nextAspect;
              onPlanChange({ ...plan, aspects: nextAspects });
            }}
            onDelete={() => {
              const nextAspects = plan.aspects.filter((_, itemIndex) => itemIndex !== index);
              onPlanChange({ ...plan, aspects: nextAspects });
            }}
          />
        ))}
      </div>

      <button className="primary-action" type="button" onClick={onRunStart} disabled={plan.aspects.length === 0}>
        开始调研
      </button>
    </div>
  );
}

export default PlanPanel;
