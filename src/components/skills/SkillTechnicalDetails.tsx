import type { ReactNode } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { formatSkillError } from "@/lib/errors/skillErrorParser";
import { cn } from "@/lib/utils";

export function getSkillTechnicalDetails(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

export function SkillTechnicalDetails({
  details,
  children,
  className,
}: {
  details?: string | null;
  children?: ReactNode;
  className?: string;
}) {
  const { t } = useTranslation();
  if (!details && !children) return null;

  return (
    <details className={cn("mt-1 text-xs text-muted-foreground", className)}>
      <summary className="cursor-pointer select-none font-medium">
        {t("skills.error.technicalDetails")}
      </summary>
      <div className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded border bg-muted/40 p-2 font-mono text-[11px]">
        {details}
        {children}
      </div>
    </details>
  );
}

export function skillDiagnosticToastOptions(details?: string | null) {
  if (!details) return undefined;
  return {
    description: <SkillTechnicalDetails details={details} />,
    duration: 10_000,
    closeButton: true,
  };
}

export function showSkillErrorToast(
  t: TFunction,
  summaryKey: string,
  error: unknown,
) {
  const details = getSkillTechnicalDetails(error);
  const formatted = formatSkillError(details, t, summaryKey);
  toast.error(formatted.title, {
    description: (
      <div>
        <p>{formatted.description}</p>
        <SkillTechnicalDetails details={formatted.technicalDetails} />
      </div>
    ),
    duration: 10_000,
    closeButton: true,
  });
}
