import { useState, useEffect } from "react";
import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogBody,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { AgentWithStatus } from "@/types";

// ─── Props ────────────────────────────────────────────────────────────────────

interface PlatformDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Pass a platform to edit it; null for create mode. */
  platform: AgentWithStatus | null;
  onAdd?: (displayName: string, globalSkillsDir: string) => Promise<void>;
  onEdit?: (displayName: string, globalSkillsDir: string) => Promise<void>;
}

// ─── PlatformDialog ───────────────────────────────────────────────────────────

export function PlatformDialog({
  open,
  onOpenChange,
  platform,
  onAdd,
  onEdit,
}: PlatformDialogProps) {
  const { t } = useTranslation();
  const isEditMode = platform !== null;

  const [displayName, setDisplayName] = useState("");
  const [globalSkillsDir, setGlobalSkillsDir] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [nameError, setNameError] = useState<string | null>(null);
  const [dirError, setDirError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Reset form when dialog opens.
  useEffect(() => {
    if (open) {
      setDisplayName(platform?.display_name ?? "");
      setGlobalSkillsDir(platform?.global_skills_dir ?? "");
      setNameError(null);
      setDirError(null);
      setError(null);
    }
  }, [open, platform]);

  async function handleSubmit() {
    const trimmedName = displayName.trim();
    const trimmedDir = globalSkillsDir.trim();

    let hasError = false;
    if (!trimmedName) {
      setNameError(t("platformDialog.nameRequired"));
      hasError = true;
    } else {
      setNameError(null);
    }
    if (!trimmedDir) {
      setDirError(t("platformDialog.dirRequired"));
      hasError = true;
    } else {
      setDirError(null);
    }

    if (hasError) return;

    setIsSubmitting(true);
    setError(null);

    try {
      if (isEditMode && onEdit) {
        await onEdit(trimmedName, trimmedDir);
      } else if (!isEditMode && onAdd) {
        await onAdd(trimmedName, trimmedDir);
      }
      onOpenChange(false);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {isEditMode ? t("platformDialog.editTitle") : t("platformDialog.addTitle")}
          </DialogTitle>
          <DialogClose />
        </DialogHeader>

        <DialogBody className="space-y-4">
          <DialogDescription>
            {isEditMode
              ? t("platformDialog.editDesc")
              : t("platformDialog.addDesc")}
          </DialogDescription>

          {/* Display name field */}
          <div className="space-y-1.5">
            <label htmlFor="platform-name" className="text-sm font-medium">
              {t("platformDialog.nameLabel")} <span className="text-destructive">*</span>
            </label>
            <Input
              id="platform-name"
              placeholder={t("platformDialog.namePlaceholder")}
              value={displayName}
              onChange={(e) => {
                setDisplayName(e.target.value);
                if (nameError) setNameError(null);
              }}
              disabled={isSubmitting}
              autoFocus
            />
            {nameError && (
              <p className="text-xs text-destructive" role="alert">
                {nameError}
              </p>
            )}
          </div>

          {/* Global skills dir field */}
          <div className="space-y-1.5">
            <label htmlFor="platform-dir" className="text-sm font-medium">
              {t("platformDialog.dirLabel")} <span className="text-destructive">*</span>
            </label>
            <Input
              id="platform-dir"
              placeholder={t("platformDialog.dirPlaceholder")}
              value={globalSkillsDir}
              onChange={(e) => {
                setGlobalSkillsDir(e.target.value);
                if (dirError) setDirError(null);
              }}
              disabled={isSubmitting}
            />
            {dirError && (
              <p className="text-xs text-destructive" role="alert">
                {dirError}
              </p>
            )}
          </div>

          {/* Backend error */}
          {error && (
            <p className="text-xs text-destructive" role="alert">
              {error}
            </p>
          )}
        </DialogBody>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={isSubmitting}
          >
            {t("platformDialog.cancel")}
          </Button>
          <Button onClick={handleSubmit} disabled={isSubmitting}>
            {isSubmitting ? (
              <>
                <Loader2 className="size-3.5 animate-spin" />
                {isEditMode ? t("platformDialog.saving") : t("platformDialog.adding")}
              </>
            ) : isEditMode ? (
              t("platformDialog.save")
            ) : (
              t("platformDialog.add")
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
