import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Select } from "./Select";

describe("Select", () => {
  it("passes value, onChange, disabled and aria-label through to a native select", () => {
    const onChange = vi.fn();
    render(
      <Select aria-label="Fruit" value="b" onChange={onChange} disabled>
        <option value="a">A</option>
        <option value="b">B</option>
      </Select>,
    );
    const select = screen.getByRole("combobox", { name: "Fruit" });
    expect(select.tagName).toBe("SELECT");
    expect(select).toHaveValue("b");
    expect(select).toBeDisabled();
    fireEvent.change(select, { target: { value: "a" } });
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("draws the house chevron and appends the layout className", () => {
    render(
      <Select aria-label="Fruit" className="w-full" defaultValue="a">
        <option value="a">A</option>
      </Select>,
    );
    const cls = screen.getByRole("combobox").className;
    expect(cls).toMatch(/appearance-none/);
    expect(cls).toMatch(/var\(--select-chevron\)/);
    expect(cls).toMatch(/border-line/);
    expect(cls).toMatch(/w-full$/);
  });

  it("insets the chevron by each size's horizontal padding", () => {
    const { rerender } = render(
      <Select aria-label="s">
        <option>A</option>
      </Select>,
    );
    const classes = () => screen.getByRole("combobox").className.split(" ");
    for (const [size, n] of [
      ["md", "3"],
      ["sm", "2"],
      ["toolbar", "2.5"],
    ] as const) {
      rerender(
        <Select aria-label="s" size={size}>
          <option>A</option>
        </Select>,
      );
      expect(classes()).toContain(`px-${n}`);
      expect(classes()).toContain(`[--select-inset:calc(var(--spacing)*${n})]`);
    }
  });
});
