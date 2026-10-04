'use client';

import { memo } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { FolderOpen, RotateCcw, X } from 'lucide-react';

import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type {
  Audio,
  Codec,
  CompressOptions,
  Engine,
  Preset,
  ToolStatus,
} from '@/lib/bindings';
import { defaultCrf, type Settings } from '@/lib/settings';
import { baseName, cn } from '@/lib/utils';

const ENGINES: { value: Engine; label: string; hint: string }[] = [
  { value: 'software', label: 'Software', hint: 'Smallest files' },
  { value: 'hardware', label: 'Hardware', hint: 'Fastest' },
];

/** Hardware encoders whose speed the preset changes. */
const PRESET_BACKENDS = ['nvenc', 'amf', 'qsv'];

const CODECS: { value: Codec; label: string; hint: string }[] = [
  { value: 'h265', label: 'H.265', hint: 'Smaller' },
  { value: 'h264', label: 'H.264', hint: 'Plays anywhere' },
];

const PRESETS: { value: Preset; label: string }[] = [
  { value: 'veryfast', label: 'Very fast' },
  { value: 'faster', label: 'Faster' },
  { value: 'fast', label: 'Fast' },
  { value: 'medium', label: 'Balanced' },
  { value: 'slow', label: 'Slow' },
  { value: 'slower', label: 'Slower' },
  { value: 'veryslow', label: 'Very slow' },
];

/** `source` stands for `null`: Radix Select needs a non-empty value. */
const FRAME_RATES = ['source', '60', '30', '25', '24'];
const RESOLUTIONS = [
  { value: 'source', label: 'Original' },
  { value: '2160', label: '2160p (4K)' },
  { value: '1440', label: '1440p' },
  { value: '1080', label: '1080p' },
  { value: '720', label: '720p' },
  { value: '480', label: '480p' },
];

const AUDIO: { value: Audio; label: string }[] = [
  { value: 'aac', label: 'AAC 128k' },
  { value: 'copy', label: 'Keep original' },
  { value: 'remove', label: 'Remove' },
];

const CRF_MIN = 12;
const CRF_MAX = 40;

/**
 * A word for a CRF, on the H.264 scale. H.265 reaches the same look about
 * five points higher, so its value is shifted before reading the word.
 */
function describeCrf(crf: number, codec: Codec): string {
  const h264 = codec === 'h265' ? crf - 5 : crf;
  if (h264 <= 17) return 'Near lossless';
  if (h264 <= 21) return 'High';
  if (h264 <= 25) return 'Balanced';
  if (h264 <= 29) return 'Small';
  return 'Tiny';
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <Label className="text-muted-foreground text-[11px] font-medium uppercase tracking-wide">
        {label}
      </Label>
      {children}
    </div>
  );
}

function toNumber(value: string): number | null {
  return value === 'source' ? null : Number(value);
}

/** Memoised: its props change only when the settings or tool status do. */
export const SettingsPanel = memo(function SettingsPanel({
  settings,
  tools,
  disabled,
  defaultImport,
  onChange,
}: {
  settings: Settings;
  tools: ToolStatus | null;
  disabled: boolean;
  /** Where card videos go when no import folder is chosen. */
  defaultImport: string | null;
  onChange: (next: Settings) => void;
}) {
  const options = settings.options;
  const set = (patch: Partial<CompressOptions>) =>
    onChange({ ...settings, options: { ...options, ...patch } });
  const crf = options.crf ?? defaultCrf(options.codec);

  const pickFolder = async (field: 'outputDir' | 'importDir') => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === 'string') set({ [field]: picked });
  };
  const importDir = options.importDir ?? defaultImport;
  const hardware = options.engine === 'hardware' ? (tools?.hardware ?? null) : null;
  // Apple's and VA-API's encoders run at one speed whatever the preset says.
  const fixedSpeed = hardware !== null && !PRESET_BACKENDS.includes(hardware.backend);

  const chooseEngine = (engine: Engine) => {
    const codecs = engine === 'hardware' ? tools?.hardware?.codecs : undefined;
    // Keep the codec if the hardware can do it, else take one it can.
    const codec =
      codecs && !codecs.includes(options.codec)
        ? (codecs[0] ?? options.codec)
        : options.codec;
    set({ engine, codec, crf: codec === options.codec ? options.crf : null });
  };

  return (
    <div className="space-y-5 px-3 py-3">
      <Field label="Encoder">
        <div className="grid grid-cols-2 gap-1.5">
          {ENGINES.map((engine) => {
            const none = engine.value === 'hardware' && tools !== null && !tools.hardware;
            return (
              <Button
                key={engine.value}
                variant={options.engine === engine.value ? 'secondary' : 'ghost'}
                disabled={disabled || none}
                onClick={() => chooseEngine(engine.value)}
                className={cn(
                  'h-auto flex-col items-start gap-0 rounded-lg border px-2.5 py-1.5',
                  options.engine === engine.value ? 'border-ring/60' : 'border-border',
                )}
              >
                <span className="text-xs font-semibold">{engine.label}</span>
                <span className="text-muted-foreground text-[10.5px] font-normal">
                  {none ? 'None found' : engine.hint}
                </span>
              </Button>
            );
          })}
        </div>
        <p className="text-muted-foreground text-[10.5px] leading-snug">
          {hardware
            ? `On ${hardware.name}. Many times faster, and frees the processor; files come out larger for the same look.`
            : 'x264 and x265 on the processor: the smallest file for the look, using every core.'}
        </p>
      </Field>

      <Field label="Codec">
        <div className="grid grid-cols-2 gap-1.5">
          {CODECS.map((codec) => {
            // Unknown until the status arrives; assume available rather than
            // flashing everything disabled at launch.
            const missing = hardware
              ? !hardware.codecs.includes(codec.value)
              : tools !== null && !tools.encoders.includes(codec.value);
            return (
              <Button
                key={codec.value}
                variant={options.codec === codec.value ? 'secondary' : 'ghost'}
                disabled={disabled || missing}
                onClick={() => set({ codec: codec.value, crf: null })}
                className={cn(
                  'h-auto flex-col items-start gap-0 rounded-lg border px-2.5 py-1.5',
                  options.codec === codec.value ? 'border-ring/60' : 'border-border',
                )}
              >
                <span className="text-xs font-semibold">{codec.label}</span>
                <span className="text-muted-foreground text-[10.5px] font-normal">
                  {missing
                    ? hardware
                      ? 'Not on this hardware'
                      : 'Not in this ffmpeg'
                    : codec.hint}
                </span>
              </Button>
            );
          })}
        </div>
      </Field>

      <Field label="Quality">
        <div className="flex items-baseline justify-between text-xs">
          <span className="font-medium">{describeCrf(crf, options.codec)}</span>
          <span className="text-muted-foreground flex items-center gap-1 font-mono text-[11px]">
            CRF {crf}
            {options.crf !== null && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() => set({ crf: null })}
                    className="hover:text-foreground"
                    aria-label="Reset to the codec's default"
                  >
                    <RotateCcw className="size-3" />
                  </button>
                </TooltipTrigger>
                <TooltipContent>Reset to {defaultCrf(options.codec)}</TooltipContent>
              </Tooltip>
            )}
          </span>
        </div>
        <Slider
          min={CRF_MIN}
          max={CRF_MAX}
          step={1}
          value={[crf]}
          disabled={disabled}
          onValueChange={([value]) => set({ crf: value })}
        />
        <div className="text-muted-foreground flex justify-between text-[10.5px]">
          <span>Better</span>
          <span>Smaller</span>
        </div>
      </Field>

      <Field label="Speed">
        <Select
          value={options.preset}
          disabled={disabled || fixedSpeed}
          onValueChange={(value) => set({ preset: value as Preset })}
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PRESETS.map((p) => (
              <SelectItem key={p.value} value={p.value}>
                {p.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-muted-foreground text-[10.5px] leading-snug">
          {fixedSpeed
            ? 'This hardware encoder runs at one speed.'
            : 'Slower finds a smaller file at the same quality.'}
        </p>
      </Field>

      <div className="grid grid-cols-2 gap-2">
        <Field label="Resolution">
          <Select
            value={options.maxResolution === null ? 'source' : String(options.maxResolution)}
            disabled={disabled}
            onValueChange={(value) => set({ maxResolution: toNumber(value) })}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {RESOLUTIONS.map((r) => (
                <SelectItem key={r.value} value={r.value}>
                  {r.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>

        <Field label="Frame rate">
          <Select
            value={options.maxFps === null ? 'source' : String(options.maxFps)}
            disabled={disabled}
            onValueChange={(value) => set({ maxFps: toNumber(value) })}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FRAME_RATES.map((fps) => (
                <SelectItem key={fps} value={fps}>
                  {fps === 'source' ? 'Original' : `Up to ${fps}`}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      </div>

      <Field label="Audio">
        <Select
          value={options.audio}
          disabled={disabled}
          onValueChange={(value) => set({ audio: value as Audio })}
        >
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {AUDIO.map((a) => (
              <SelectItem key={a.value} value={a.value}>
                {a.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>

      <Field label="Save to">
        <div className="flex items-center gap-1.5">
          <Button
            variant="outline"
            size="sm"
            disabled={disabled}
            onClick={() => void pickFolder('outputDir')}
            className="min-w-0 flex-1 justify-start"
            title={options.outputDir ?? undefined}
          >
            <FolderOpen />
            <span className="truncate">
              {options.outputDir ? baseName(options.outputDir) : 'Next to originals'}
            </span>
          </Button>
          {options.outputDir && (
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={disabled}
              onClick={() => set({ outputDir: null })}
              aria-label="Save next to originals"
            >
              <X />
            </Button>
          )}
        </div>
        <p className="text-muted-foreground text-[10.5px] leading-snug">
          {options.outputDir
            ? 'Named after the original, as .mp4.'
            : 'Saved as name-compressed.mp4 beside each original.'}
        </p>
      </Field>

      <Field label="Camera cards">
        <div className="flex items-center gap-1.5">
          <Button
            variant="outline"
            size="sm"
            disabled={disabled || options.outputDir !== null}
            onClick={() => void pickFolder('importDir')}
            className="min-w-0 flex-1 justify-start"
            title={importDir ?? undefined}
          >
            <FolderOpen />
            <span className="truncate">{importDir ? baseName(importDir) : 'Choose a folder'}</span>
          </Button>
          {options.importDir && (
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={disabled}
              onClick={() => set({ importDir: null })}
              aria-label="Use the default import folder"
            >
              <X />
            </Button>
          )}
        </div>
        <p className="text-muted-foreground text-[10.5px] leading-snug">
          {options.outputDir
            ? 'Card videos go to the Save to folder above.'
            : 'Read straight from the card and saved here, never back to the card.'}
        </p>
        <label className="flex items-center justify-between gap-2 pt-1 text-xs">
          <span>Compress new cards automatically</span>
          <Switch
            size="sm"
            checked={settings.autoImport}
            onCheckedChange={(autoImport) => onChange({ ...settings, autoImport })}
          />
        </label>
      </Field>

      <div className="space-y-3">
        <label className="flex items-center justify-between gap-2 text-xs">
          <span>Replace existing outputs</span>
          <Switch
            size="sm"
            checked={options.overwrite}
            disabled={disabled}
            onCheckedChange={(overwrite) => set({ overwrite })}
          />
        </label>
        <label className="flex items-center justify-between gap-2 text-xs">
          <span>Chime when done</span>
          <Switch
            size="sm"
            checked={settings.chime}
            onCheckedChange={(chime) => onChange({ ...settings, chime })}
          />
        </label>
      </div>
    </div>
  );
});
