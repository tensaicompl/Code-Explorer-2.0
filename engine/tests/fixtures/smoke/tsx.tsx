export function Hello(props: { name: string }) {
  return <div>{props.name}</div>;
}
export const App = () => <Hello name="x" />;
