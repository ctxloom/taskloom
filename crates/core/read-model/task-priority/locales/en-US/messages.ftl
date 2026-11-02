# Messages for the crate's errors. Each id is an error's code without the
# crate's name, which the build prefixes (angzarr_diagnostic::build); the
# variables are its args.

malformed = { $facet } on { $target } is not a formula: { $reason } (formula: { $formula })
unknown-builtin = { $facet } on { $target } reads { $name }, which is no tag (no ':') and no built-in of { $facet } (those are { $known })
ambiguous = more than one { $facet } is declared (on { $targets }): exactly one is allowed
unevaluable = { $facet } could not score task { $harp_id }: { $reason } (formula: { $formula })
not-a-number = { $facet } gives task { $harp_id } { $result }, which is no number (formula: { $formula })
non-finite = priority_fn gives task { $harp_id } the score { $score }, which is no finite number: check its tags for a value that is not one
no-created-at = task { $harp_id } has no creation time, so it has no age to rank by
