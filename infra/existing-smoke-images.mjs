const validImageId = value => typeof value === 'string' && value.length === 71 && /^sha256:[a-f0-9]{64}$/.test(value);

// Validate every supplied override before inspecting images or creating test resources.
export function smokeImageReferences(environment, prefix, defaults) {
  return Object.entries(defaults).map(([name, fallback]) => {
    const key = `${prefix}_${name.toUpperCase()}_IMAGE`;
    const override = environment[key];
    if (override !== undefined && !validImageId(override))
      throw new Error(`${key} must be an exact lowercase sha256 Docker image ID.`);
    return { name, key, reference: override ?? fallback, explicit: override !== undefined };
  });
}

export async function pinExistingSmokeImages(references, inspect) {
  const images = {};
  for (const input of references) {
    const output = await inspect(input.reference);
    const image = typeof output === 'string' ? output.trim() : undefined;
    if (!validImageId(image)) throw new Error(`Could not pin the required existing image for ${input.key}.`);
    if (input.explicit && image !== input.reference)
      throw new Error(`The existing image does not match the explicit immutable ID for ${input.key}.`);
    images[input.name] = image;
  }
  return images;
}
