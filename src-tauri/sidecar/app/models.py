from pydantic import BaseModel


class SearchRequest(BaseModel):
    query: str
    limit: int = 8


class EnrichRequest(BaseModel):
    url: str


class SearchResultItem(BaseModel):
    title: str
    url: str
    content: str
    provider: str
    source: str = "sidecar_academic"
    category: str
    authors: str | None = None
    publish_year: int | None = None
    keywords: str | None = None
    relevance_score: float = 0.0
    raw_json: dict = {}


class SearchResponse(BaseModel):
    items: list[SearchResultItem]
    warning: str | None = None
