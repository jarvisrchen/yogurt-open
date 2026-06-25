import { useEffect, useRef, useState } from "react";
import { Search } from "lucide-react";

interface SearchPillProps {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  width?: string;
}

export function SearchPill({
  value,
  onChange,
  placeholder = "Search notes & transcripts",
  width = "w-70",
}: SearchPillProps) {
  const [local, setLocal] = useState(value);
  const sent = useRef(value);

  // Follow parent-forced changes such as clearing on route change. The echo of our own text can land after
  // another keystroke, since the URL commits in a transition, so it must not overwrite the box.
  useEffect(() => {
    if (value !== sent.current) setLocal(value);
    sent.current = value;
  }, [value]);

  useEffect(() => {
    if (local === value) return;
    const id = setTimeout(() => {
      sent.current = local;
      onChange(local);
    }, 200);
    return () => clearTimeout(id);
  }, [local, value, onChange]);

  return (
    <div
      role="search"
      className={`flex items-center gap-2 bg-card border border-line rounded-pill px-4 py-2 text-ui text-ink ${width} focus-within:ring-2 focus-within:ring-blue/40`}
    >
      <Search size={16} className="text-mut shrink-0" aria-hidden />
      <input
        type="text"
        value={local}
        onChange={(e) => setLocal(e.target.value)}
        placeholder={placeholder}
        aria-label={placeholder.replace("&", "and")}
        className="flex-1 bg-transparent outline-none placeholder:text-mut"
      />
    </div>
  );
}
