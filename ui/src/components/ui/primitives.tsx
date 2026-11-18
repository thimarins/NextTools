import { forwardRef, type HTMLAttributes, type InputHTMLAttributes, type LabelHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

export function Card({ className, ...p }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("rounded-xl border bg-card text-card-foreground shadow-[0_1px_2px_rgb(0_0_0/0.04)]", className)} {...p} />;
}

export function CardHeader({ title, description, actions, icon }: { title: ReactNode; description?: ReactNode; actions?: ReactNode; icon?: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-4 px-[var(--card-pad)] pt-[var(--card-pad)] pb-3">
      <div className="flex min-w-0 items-start gap-3">
        {icon && <div className="mt-0.5 text-muted-foreground [&_svg]:size-[18px]">{icon}</div>}
        <div className="min-w-0">
          <h3 className="text-[15px] font-semibold leading-tight">{title}</h3>
          {description && <p className="mt-1 text-sm text-muted-foreground">{description}</p>}
        </div>
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}

export function CardBody({ className, ...p }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("px-[var(--card-pad)] pb-[var(--card-pad)]", className)} {...p} />;
}

const badgeVariants = cva("inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium whitespace-nowrap [&_svg]:size-3", {
  variants: {
    tone: {
      neutral: "border-border bg-muted text-muted-foreground",
      primary: "border-transparent bg-accent text-accent-foreground",
      success: "border-transparent bg-success/15 text-success",
      warning: "border-transparent bg-warning/18 text-[color-mix(in_oklch,var(--warning),var(--foreground)_35%)]",
      danger: "border-transparent bg-destructive/12 text-destructive",
      info: "border-transparent bg-info/14 text-info",
    },
  },
  defaultVariants: { tone: "neutral" },
});

export function Badge({ className, tone, ...p }: HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone }), className)} {...p} />;
}

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(({ className, ...p }, ref) => (
  <input
    ref={ref}
    className={cn(
      "h-[var(--control-h)] w-full min-w-0 rounded-md border border-input bg-card px-3 text-sm placeholder:text-muted-foreground/70 focus-visible:outline-2 disabled:opacity-50",
      className,
    )}
    {...p}
  />
));
Input.displayName = "Input";

export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(({ className, children, ...p }, ref) => (
  <select ref={ref} className={cn("h-[var(--control-h)] w-full rounded-md border border-input bg-card px-2.5 text-sm disabled:opacity-50", className)} {...p}>
    {children}
  </select>
));
Select.displayName = "Select";

export function Label({ className, ...p }: LabelHTMLAttributes<HTMLLabelElement>) {
  return <label className={cn("text-sm font-medium", className)} {...p} />;
}

export function Field({ label, hint, children, htmlFor }: { label: ReactNode; hint?: ReactNode; children: ReactNode; htmlFor?: string }) {
  return (
    <div className="grid gap-1.5">
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

/** Determinate when `value` is a number 0..1; indeterminate otherwise (never a fake %). */
export function Progress({ value, className, tone = "primary" }: { value?: number | null; className?: string; tone?: "primary" | "warning" | "danger" }) {
  const color = tone === "warning" ? "bg-warning" : tone === "danger" ? "bg-destructive" : "bg-primary";
  return (
    <div className={cn("relative h-1.5 w-full overflow-hidden rounded-full bg-muted", className)} role="progressbar" aria-valuenow={value != null ? Math.round(value * 100) : undefined}>
      {value != null ? (
        <div className={cn("h-full rounded-full transition-[width] duration-500", color)} style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }} />
      ) : (
        <div className={cn("absolute inset-y-0 w-2/5 rounded-full", color)} style={{ animation: "indeterminate 1.6s ease-in-out infinite" }} />
      )}
    </div>
  );
}

export function Empty({ icon, title, children }: { icon?: ReactNode; title: ReactNode; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-10 text-center">
      {icon && <div className="mb-1 rounded-full bg-muted p-3 text-muted-foreground [&_svg]:size-5">{icon}</div>}
      <p className="text-sm font-medium">{title}</p>
      {children && <div className="max-w-sm text-sm text-muted-foreground">{children}</div>}
    </div>
  );
}

export function Stat({ label, value, sub }: { label: ReactNode; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="min-w-0">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      <p className="mt-1 truncate text-xl font-semibold tracking-tight">{value}</p>
      {sub && <p className="mt-0.5 truncate text-xs text-muted-foreground">{sub}</p>}
    </div>
  );
}

export function PageHeader({ title, description, actions }: { title: ReactNode; description?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        {description && <p className="mt-1 text-sm text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}
