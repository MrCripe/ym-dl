import { useState, type CSSProperties, type ReactNode } from "react";

const FILLED: CSSProperties = {
  fontVariationSettings: '"FILL" 1, "wght" 500, "GRAD" 0, "opsz" 24',
};

export function Icon({
  name,
  filled = false,
  className = "",
  size,
}: {
  name: string;
  filled?: boolean;
  className?: string;
  size?: number;
}) {
  return (
    <span
      aria-hidden
      className={`material-symbols-rounded ${className}`.trim()}
      style={{ ...(filled ? FILLED : undefined), ...(size ? { fontSize: size } : undefined) }}
    >
      {name}
    </span>
  );
}

/** Material 3 ripple: the visual answer to every click. */
export function useRipple() {
  return (event: React.MouseEvent<HTMLElement>) => {
    const host = event.currentTarget;
    const rect = host.getBoundingClientRect();
    const diameter = Math.max(rect.width, rect.height) * 1.6;
    const ripple = document.createElement("span");
    ripple.className = "ripple";
    ripple.style.width = `${diameter}px`;
    ripple.style.height = `${diameter}px`;
    ripple.style.left = `${event.clientX - rect.left - diameter / 2}px`;
    ripple.style.top = `${event.clientY - rect.top - diameter / 2}px`;
    host.appendChild(ripple);
    window.setTimeout(() => ripple.remove(), 560);
  };
}

type ButtonProps = {
  children?: ReactNode;
  icon?: string;
  variant?: "filled" | "tonal" | "outlined" | "text" | "elevated" | "danger" | "danger-tonal";
  size?: "compact" | "default" | "large";
  block?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  type?: "button" | "submit";
  className?: string;
};

export function Button({
  children,
  icon,
  variant = "filled",
  size = "default",
  block,
  disabled,
  onClick,
  type = "button",
  className = "",
}: ButtonProps) {
  const ripple = useRipple();
  return (
    <button
      type={type}
      className={`btn ${variant} ${size === "default" ? "" : size} ${block ? "block" : ""} ${className}`.trim()}
      disabled={disabled}
      onClick={(event) => {
        ripple(event);
        onClick?.();
      }}
    >
      {icon ? <Icon name={icon} /> : null}
      {children}
    </button>
  );
}

export function IconButton({
  icon,
  label,
  variant = "standard",
  size = "default",
  filled = false,
  disabled,
  onClick,
  className = "",
}: {
  icon: string;
  label: string;
  variant?: "standard" | "filled" | "tonal" | "outlined";
  size?: "default" | "small";
  filled?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  className?: string;
}) {
  const ripple = useRipple();
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      className={`icon-btn ${variant === "standard" ? "" : variant} ${size === "small" ? "small" : ""} ${className}`.trim()}
      onClick={(event) => {
        ripple(event);
        onClick?.();
      }}
    >
      <Icon name={icon} filled={filled} />
    </button>
  );
}

export function TextField({
  label,
  value,
  placeholder,
  onChange,
  type = "text",
  icon,
  trailing,
  hint,
  error,
  autoFocus,
  onEnter,
  disabled,
}: {
  label?: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
  type?: "text" | "password";
  icon?: string;
  trailing?: ReactNode;
  hint?: string;
  error?: string;
  autoFocus?: boolean;
  onEnter?: () => void;
  disabled?: boolean;
}) {
  const [focused, setFocused] = useState(false);
  return (
    <div className={`field ${focused ? "focused" : ""} ${error ? "error" : ""}`}>
      {label ? <label>{label}</label> : null}
      <div className="control">
        {icon ? (
          <span className="leading">
            <Icon name={icon} />
          </span>
        ) : null}
        <input
          type={type}
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          autoFocus={autoFocus}
          spellCheck={false}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") onEnter?.();
          }}
        />
        {trailing ? <span className="trailing">{trailing}</span> : null}
      </div>
      {error || hint ? <span className="hint">{error || hint}</span> : null}
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="switch"
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="track" />
      <span className="thumb" />
    </button>
  );
}

export function Checkbox({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      className="checkbox"
      disabled={disabled}
      onClick={(event) => {
        event.stopPropagation();
        onChange(!checked);
      }}
    >
      <Icon name="check" />
    </button>
  );
}

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  disabled,
  label,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (value: number) => void;
  disabled?: boolean;
  label: string;
}) {
  const fill = ((value - min) / Math.max(1, max - min)) * 100;
  return (
    <div className="slider" style={{ ["--fill" as string]: `${fill}%` }}>
      <input
        type="range"
        aria-label={label}
        value={value}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onChange={(event) => onChange(Number(event.target.value))}
      />
    </div>
  );
}

export function Segmented<T extends string | number>({
  options,
  value,
  onChange,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="segmented" role="group">
      {options.map((option) => (
        <button
          key={String(option.value)}
          type="button"
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Badge({
  children,
  tone = "neutral",
  icon,
}: {
  children: ReactNode;
  tone?: "primary" | "secondary" | "error" | "success" | "warning" | "neutral";
  icon?: string;
}) {
  return (
    <span className={`badge ${tone}`}>
      {icon ? <Icon name={icon} filled /> : null}
      {children}
    </span>
  );
}

export function Progress({ value, indeterminate }: { value: number; indeterminate?: boolean }) {
  return (
    <div className={`progress ${indeterminate ? "indeterminate" : ""}`}>
      <div className="bar" style={{ width: `${Math.min(100, Math.max(0, value))}%` }} />
    </div>
  );
}
