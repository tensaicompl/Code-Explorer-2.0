import * as React from "react";

type Field = { name: string; label: string; required?: boolean };

interface FormState {
  values: Record<string, string>;
  errors: Partial<Record<string, string>>;
}

export class Form extends React.Component<{ fields: Field[] }, FormState> {
  state: FormState = { values: {}, errors: {} };

  private change = (name: string) => (event: React.ChangeEvent<HTMLInputElement>) => {
    const value = event.target.value;
    this.setState((prev) => ({ values: { ...prev.values, [name]: value }, errors: prev.errors }));
  };

  render() {
    const { fields } = this.props;
    return (
      <form onSubmit={(e) => e.preventDefault()}>
        {fields.map(({ name, label, required }) => (
          <label key={name}>
            {label}
            {required && <span aria-hidden="true">*</span>}
            <input name={name} value={this.state.values[name] ?? ""} onChange={this.change(name)} />
            {this.state.errors[name] ? <em role="alert">{this.state.errors[name]}</em> : null}
          </label>
        ))}
        <button type="submit" disabled={fields.length === 0}>
          Send
        </button>
      </form>
    );
  }
}
