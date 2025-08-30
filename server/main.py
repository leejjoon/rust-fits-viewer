# server/main.py
from fastapi import FastAPI
from .routes import router

app = FastAPI()

app.include_router(router)

@app.get("/")
def read_root():
    return {"message": "FITS-View server is running"}
