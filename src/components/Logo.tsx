/**
 * The application mark: a play triangle cut into three pieces that fade as
 * they go — the video getting lighter.
 *
 * This is the trimmed artwork, with no background tile and the viewBox
 * cropped to the triangle, so it sits on whatever surface it is placed on.
 * The installer icons in `src-tauri/icons` are the same paths on a full
 * square, which carries the margin and background a desktop icon needs.
 *
 * The canvas is taller than it is wide, so size this with a height and let
 * the width follow (`h-5 w-auto`).
 */
export function Logo({ className }: { className?: string }) {
  return (
    <svg
      viewBox="350 235 480 554"
      xmlns="http://www.w3.org/2000/svg"
      role="img"
      aria-label="karui"
      className={className}
    >
      <path className="fill-foreground" d="M350 235L520 333.1V690.9L350 789Z" />
      <path className="fill-foreground/70" d="M560 356.2L680 425.4V598.6L560 667.8Z" />
      <path className="fill-foreground/45" d="M715 445.6L830 512L715 578.4Z" />
    </svg>
  );
}
