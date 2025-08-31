# FITS Viewer Server

A **FastAPI-based Python application** that serves FITS (Flexible Image Transport System) image data as tiles for visualization.

## Components

- **`main.py`**: FastAPI application entry point with root endpoint
- **`routes.py`**: Core API endpoints for metadata and tile serving
- **`header.py`**: Binary header format for raw tile data
- **`requirements.txt`**: Python dependencies

## API Endpoints

- **`GET /`**: Health check endpoint
- **`GET /meta`**: Returns image metadata (shape, dtype, tile size, zoom levels)
- **`GET /tile/{z}/{x}/{y}`**: Returns image tiles in raw binary format with custom headers

## How to Run

### 1. Install Dependencies
```bash
cd server
pip install -r requirements.txt
```

### 2. Start the Server
```bash
# From the server directory
uvicorn main:app --reload --host 0.0.0.0 --port 8000

# Or from the project root
uvicorn server.main:app --reload --host 0.0.0.0 --port 8000
```

### 3. Test the Server
- **Health check**: `http://localhost:8000/`
- **Metadata**: `http://localhost:8000/meta`
- **Sample tile**: `http://localhost:8000/tile/0/0/0`

## Current Implementation

The server currently uses a **synthetic 1024x1024 numpy array** as test data. It serves 256x256 pixel tiles with a custom binary format that includes a 32-byte header followed by raw float32 pixel data.

## Dependencies

- `fastapi`: Web framework
- `uvicorn`: ASGI server
- `numpy`: Array operations
- `astropy`: FITS file handling (for future use)
