# Corpus-only config: anonymous visitors get a Gamma-shaped Public role so
# the harvester never runs a login flow (grafana gets the same treatment via
# env vars). Anonymous-as-Admin 500s — superset dereferences the user object
# on pages that attribute ownership, and anonymous has none to give.
# NEVER deploy anything resembling this outside a disposable local corpus.
PUBLIC_ROLE_LIKE = "Gamma"
SECRET_KEY = "verbivore-corpus-not-a-secret"
