import type { AspectDto } from "../api/types";

interface PlanCardProps {
  aspect: AspectDto;
  index: number;
  onChange: (aspect: AspectDto) => void;
  onDelete: () => void;
}

function PlanCard({ aspect, index, onChange, onDelete }: PlanCardProps) {
  return (
    <article className="plan-card">
      <div className="plan-card-header">
        <span className="step-number">{index + 1}</span>
        <input
          className="title-input"
          aria-label={`搜索方向 ${index + 1} 名称`}
          value={aspect.name_zh}
          onChange={(event) => onChange({ ...aspect, name_zh: event.target.value })}
        />
        <button
          className="icon-button"
          type="button"
          onClick={onDelete}
          title="删除方向"
          aria-label={`删除搜索方向 ${index + 1}`}
        >
          x
        </button>
      </div>

      <label className="field compact">
        <span>研究意图</span>
        <textarea
          value={aspect.rationale_zh}
          rows={3}
          onChange={(event) => onChange({ ...aspect, rationale_zh: event.target.value })}
        />
      </label>

      <label className="field compact">
        <span>GitHub query</span>
        <input
          value={aspect.github_query}
          onChange={(event) => onChange({ ...aspect, github_query: event.target.value })}
        />
      </label>

      <label className="field compact">
        <span>arXiv query</span>
        <input
          value={aspect.arxiv_query}
          onChange={(event) => onChange({ ...aspect, arxiv_query: event.target.value })}
        />
      </label>

      <div className="limit-grid">
        <label className="field compact">
          <span>GitHub limit</span>
          <input
            min={1}
            type="number"
            value={aspect.github_limit}
            onChange={(event) =>
              onChange({ ...aspect, github_limit: Number(event.target.value) })
            }
          />
        </label>
        <label className="field compact">
          <span>arXiv limit</span>
          <input
            min={1}
            type="number"
            value={aspect.arxiv_limit}
            onChange={(event) =>
              onChange({ ...aspect, arxiv_limit: Number(event.target.value) })
            }
          />
        </label>
      </div>
    </article>
  );
}

export default PlanCard;
