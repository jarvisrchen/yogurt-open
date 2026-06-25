import { render, screen } from "@testing-library/react";
import { Slot } from "./slots";

const A = ({ x }: { x: number }) => <p>A{x}</p>;
const B = ({ x }: { x: number }) => <p>B{x}</p>;

describe("Slot", () => {
  it("renders each component with the props", () => {
    render(<Slot of={[A, B]} x={1} />);
    expect(screen.getByText("A1")).toBeInTheDocument();
    expect(screen.getByText("B1")).toBeInTheDocument();
  });

  it("renders nothing for an empty slot", () => {
    const { container } = render(<Slot of={[]} />);
    expect(container).toBeEmptyDOMElement();
  });
});
