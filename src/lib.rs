#[macro_use]
extern crate derivative;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate serde_derive;

use chrono::prelude::*;
use chrono::Duration;
use failure::Error;
use failure::ResultExt;
use serde::de::{self, Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "read-url")]
use std::io::Read;

pub trait Id {
    fn id(&self) -> &str;
}

pub trait Type {
    fn object_type(&self) -> ObjectType;
}

#[derive(Debug, Serialize, Eq, PartialEq)]
pub enum ObjectType {
    Agency,
    Stop,
    Route,
    Trip,
    Calendar,
    Shape,
    Fare,
}

#[derive(Fail, Debug)]
#[fail(display = "The id {} is not known", id)]
pub struct ReferenceError {
    pub id: String,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum LocationType {
    StopPoint = 0,
    StopArea = 1,
    StationEntrance = 2,
}

impl Default for LocationType {
    fn default() -> LocationType {
        LocationType::StopPoint
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum RouteType {
    Tramway,
    Subway,
    Rail,
    Bus,
    Ferry,
    CableCar,
    Gondola,
    Funicular,
    // Any other value than 0..7 is invalid in the GTFS
    // However, some bad files might have other values
    // We don’t want to stop nor skip too soon during deserialization
    Other(u16),
}

impl Default for RouteType {
    fn default() -> RouteType {
        RouteType::Bus
    }
}

impl<'de> ::serde::Deserialize<'de> for RouteType {
    fn deserialize<D>(deserializer: D) -> Result<RouteType, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        let i = u16::deserialize(deserializer)?;
        Ok(match i {
            0 => RouteType::Tramway,
            1 => RouteType::Subway,
            2 => RouteType::Rail,
            3 => RouteType::Bus,
            4 => RouteType::Ferry,
            5 => RouteType::CableCar,
            6 => RouteType::Gondola,
            7 => RouteType::Funicular,
            _ => RouteType::Other(i),
        })
    }
}

#[derive(Derivative)]
#[derivative(Default(bound = ""))]
#[derive(Debug, Deserialize, Copy, Clone, PartialEq)]
pub enum PickupDropOffType {
    #[derivative(Default)]
    #[serde(rename = "0")]
    Regular,
    #[serde(rename = "1")]
    NotAvailable,
    #[serde(rename = "2")]
    ArrangeByPhone,
    #[serde(rename = "3")]
    CoordinateWithDriver,
}

#[derive(Debug, Deserialize)]
pub struct Calendar {
    #[serde(rename = "service_id")]
    pub id: String,
    #[serde(deserialize_with = "deserialize_bool")]
    pub monday: bool,
    #[serde(deserialize_with = "deserialize_bool")]
    pub tuesday: bool,
    #[serde(deserialize_with = "deserialize_bool")]
    pub wednesday: bool,
    #[serde(deserialize_with = "deserialize_bool")]
    pub thursday: bool,
    #[serde(deserialize_with = "deserialize_bool")]
    pub friday: bool,
    #[serde(deserialize_with = "deserialize_bool")]
    pub saturday: bool,
    #[serde(deserialize_with = "deserialize_bool")]
    pub sunday: bool,
    #[serde(deserialize_with = "deserialize_date")]
    pub start_date: NaiveDate,
    #[serde(deserialize_with = "deserialize_date")]
    pub end_date: NaiveDate,
}

impl Type for Calendar {
    fn object_type(&self) -> ObjectType {
        ObjectType::Calendar
    }
}

impl Id for Calendar {
    fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for Calendar {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}—{}", self.start_date, self.end_date)
    }
}

#[derive(Serialize, Deserialize, Debug, Derivative, PartialEq, Eq, Hash, Clone, Copy)]
#[derivative(Default)]
pub enum Availability {
    #[derivative(Default)]
    #[serde(rename = "0")]
    InformationNotAvailable,
    #[serde(rename = "1")]
    Available,
    #[serde(rename = "2")]
    NotAvailable,
}

impl Calendar {
    pub fn valid_weekday(&self, date: NaiveDate) -> bool {
        match date.weekday() {
            Weekday::Mon => self.monday,
            Weekday::Tue => self.tuesday,
            Weekday::Wed => self.wednesday,
            Weekday::Thu => self.thursday,
            Weekday::Fri => self.friday,
            Weekday::Sat => self.saturday,
            Weekday::Sun => self.sunday,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CalendarDate {
    pub service_id: String,
    #[serde(deserialize_with = "deserialize_date")]
    pub date: NaiveDate,
    pub exception_type: u8,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Stop {
    #[serde(rename = "stop_id")]
    pub id: String,
    #[serde(rename = "stop_code")]
    pub code: Option<String>,
    #[serde(rename = "stop_name")]
    pub name: String,
    #[serde(default, rename = "stop_desc")]
    pub description: String,
    #[serde(
        deserialize_with = "deserialize_location_type",
        default = "default_location_type"
    )]
    pub location_type: LocationType,
    pub parent_station: Option<String>,
    #[serde(deserialize_with = "de_with_trimed_float")]
    #[serde(rename = "stop_lon", default)]
    pub longitude: f64,
    #[serde(deserialize_with = "de_with_trimed_float")]
    #[serde(rename = "stop_lat", default)]
    pub latitude: f64,
    #[serde(rename = "stop_timezone")]
    pub timezone: Option<String>,
    #[serde(deserialize_with = "de_with_empty_default", default)]
    pub wheelchair_boarding: Availability,
}

impl Type for Stop {
    fn object_type(&self) -> ObjectType {
        ObjectType::Stop
    }
}

impl Id for Stop {
    fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct RawStopTime {
    trip_id: String,
    #[serde(deserialize_with = "deserialize_time")]
    pub arrival_time: u32,
    #[serde(deserialize_with = "deserialize_time")]
    pub departure_time: u32,
    stop_id: String,
    stop_sequence: u16,
    pickup_type: Option<PickupDropOffType>,
    drop_off_type: Option<PickupDropOffType>,
}

#[derive(Debug, Default)]
pub struct StopTime {
    pub arrival_time: u32,
    pub stop: Arc<Stop>,
    pub departure_time: u32,
    pub pickup_type: Option<PickupDropOffType>,
    pub drop_off_type: Option<PickupDropOffType>,
    pub stop_sequence: u16,
}

impl StopTime {
    fn from(stop_time_gtfs: &RawStopTime, stop: Arc<Stop>) -> Self {
        Self {
            arrival_time: stop_time_gtfs.arrival_time,
            departure_time: stop_time_gtfs.departure_time,
            stop,
            pickup_type: stop_time_gtfs.pickup_type,
            drop_off_type: stop_time_gtfs.drop_off_type,
            stop_sequence: stop_time_gtfs.stop_sequence,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Route {
    #[serde(rename = "route_id")]
    pub id: String,
    #[serde(rename = "route_short_name")]
    pub short_name: String,
    #[serde(rename = "route_long_name")]
    pub long_name: String,
    pub route_type: RouteType,
    pub agency_id: Option<String>,
    pub route_order: Option<u32>,
}

impl Type for Route {
    fn object_type(&self) -> ObjectType {
        ObjectType::Route
    }
}

impl Id for Route {
    fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.long_name.is_empty() {
            write!(f, "{}", self.long_name)
        } else {
            write!(f, "{}", self.short_name)
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct RawTrip {
    #[serde(rename = "trip_id")]
    pub id: String,
    pub service_id: String,
    pub route_id: String,
}

impl Type for RawTrip {
    fn object_type(&self) -> ObjectType {
        ObjectType::Trip
    }
}

impl Id for RawTrip {
    fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for RawTrip {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "route id: {}, service id: {}",
            self.route_id, self.service_id
        )
    }
}

#[derive(Debug, Default)]
pub struct Trip {
    pub id: String,
    pub service_id: String,
    pub route_id: String,
    pub stop_times: Vec<StopTime>,
}

impl Type for Trip {
    fn object_type(&self) -> ObjectType {
        ObjectType::Trip
    }
}

impl Id for Trip {
    fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for Trip {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "route id: {}, service id: {}",
            self.route_id, self.service_id
        )
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Agency {
    #[serde(rename = "agency_id")]
    pub id: Option<String>,
    #[serde(rename = "agency_name")]
    pub name: String,
    #[serde(rename = "agency_url")]
    pub url: String,
    #[serde(rename = "agency_timezone")]
    pub timezone: String,
    #[serde(rename = "agency_lang")]
    pub lang: Option<String>,
    #[serde(rename = "agency_phone")]
    pub phone: Option<String>,
    #[serde(rename = "agency_fare_url")]
    pub fare_url: Option<String>,
    #[serde(rename = "agency_email")]
    pub email: Option<String>,
}

impl Type for Agency {
    fn object_type(&self) -> ObjectType {
        ObjectType::Agency
    }
}

impl Id for Agency {
    fn id(&self) -> &str {
        match &self.id {
            None => "",
            Some(id) => id,
        }
    }
}

impl fmt::Display for Agency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Shape {
    #[serde(rename = "shape_id")]
    pub id: String,
    #[serde(rename = "shape_pt_lat", default)]
    pub latitude: f64,
    #[serde(rename = "shape_pt_lon", default)]
    pub longitude: f64,
    #[serde(rename = "shape_pt_sequence")]
    pub sequence: usize,
    #[serde(rename = "shape_dist_traveled")]
    pub dist_traveled: Option<f32>,
}

impl Type for Shape {
    fn object_type(&self) -> ObjectType {
        ObjectType::Shape
    }
}

impl Id for Shape {
    fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Deserialize)]
pub struct FareAttribute {
    #[serde(rename = "fare_id")]
    pub id: String,
    pub price: String,
    #[serde(rename = "currency_type")]
    pub currency: String,
    pub payment_method: PaymentMethod,
    pub transfers: Transfers,
    pub agency_id: Option<String>,
    pub transfer_duration: Option<usize>,
}

impl Id for FareAttribute {
    fn id(&self) -> &str {
        &self.id
    }
}

impl Type for FareAttribute {
    fn object_type(&self) -> ObjectType {
        ObjectType::Fare
    }
}

#[derive(Debug, Deserialize, Copy, Clone, PartialEq)]
pub enum PaymentMethod {
    #[serde(rename = "0")]
    Aboard,
    #[serde(rename = "1")]
    PreBoarding,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Transfers {
    Unlimited,
    NoTransfer,
    UniqueTransfer,
    TwoTransfers,
    Other(u16),
}

impl<'de> ::serde::Deserialize<'de> for Transfers {
    fn deserialize<D>(deserializer: D) -> Result<Transfers, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        let i = Option::<u16>::deserialize(deserializer)?;
        Ok(match i {
            Some(0) => Transfers::NoTransfer,
            Some(1) => Transfers::UniqueTransfer,
            Some(2) => Transfers::TwoTransfers,
            Some(a) => Transfers::Other(a),
            None => Transfers::default(),
        })
    }
}

impl Default for Transfers {
    fn default() -> Transfers {
        Transfers::Unlimited
    }
}

#[derive(Debug, Deserialize)]
pub struct FeedInfo {
    #[serde(rename = "feed_publisher_name")]
    pub name: String,
    #[serde(rename = "feed_publisher_url")]
    pub url: String,
    #[serde(rename = "feed_lang")]
    pub lang: String,
    #[serde(
        deserialize_with = "deserialize_option_date",
        rename = "feed_start_date"
    )]
    pub start_date: Option<NaiveDate>,
    #[serde(deserialize_with = "deserialize_option_date", rename = "feed_end_date")]
    pub end_date: Option<NaiveDate>,
    #[serde(rename = "feed_version")]
    pub version: Option<String>,
}

impl fmt::Display for FeedInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn deserialize_date<'de, D>(deserializer: D) -> Result<NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&s, "%Y%m%d").map_err(serde::de::Error::custom)
}

fn deserialize_option_date<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = Option::<String>::deserialize(deserializer)?
        .map(|s| NaiveDate::parse_from_str(&s, "%Y%m%d").map_err(serde::de::Error::custom));
    match s {
        Some(Ok(s)) => Ok(Some(s)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn parse_time(s: &str) -> Result<u32, Error> {
    let v: Vec<&str> = s.split(':').collect();
    Ok(&v[0].parse()? * 3600u32 + &v[1].parse()? * 60u32 + &v[2].parse()?)
}

fn deserialize_time<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    parse_time(&s).map_err(de::Error::custom)
}

fn deserialize_location_type<'de, D>(deserializer: D) -> Result<LocationType, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    Ok(match s.as_str() {
        "1" => LocationType::StopArea,
        "2" => LocationType::StationEntrance,
        _ => LocationType::StopPoint,
    })
}

fn de_with_trimed_float<'de, D>(de: D) -> Result<f64, D::Error>
where
    D: ::serde::Deserializer<'de>,
{
    String::deserialize(de).and_then(|s| s.trim().parse().map_err(de::Error::custom))
}

pub fn de_with_empty_default<'de, T: Default, D>(de: D) -> Result<T, D::Error>
where
    D: ::serde::Deserializer<'de>,
    T: ::serde::Deserialize<'de>,
{
    use serde::Deserialize;
    Option::<T>::deserialize(de).map(|opt| opt.unwrap_or_else(Default::default))
}

fn default_location_type() -> LocationType {
    LocationType::StopPoint
}

fn read_objs<T, O>(reader: T) -> Result<Vec<O>, Error>
where
    for<'de> O: Deserialize<'de>,
    T: std::io::Read,
{
    Ok(csv::Reader::from_reader(reader)
        .deserialize()
        .collect::<Result<_, _>>()?)
}

#[derive(Default)]
pub struct RawGtfs {
    pub read_duration: i64,
    pub calendar: Vec<Calendar>,
    pub calendar_dates: Vec<CalendarDate>,
    pub stops: Vec<Stop>,
    pub routes: Vec<Route>,
    pub trips: Vec<RawTrip>,
    pub agencies: Vec<Agency>,
    pub shapes: Vec<Shape>,
    pub fare_attributes: Vec<FareAttribute>,
    pub feed_info: Vec<FeedInfo>,
    pub stop_times: Vec<RawStopTime>,
}

#[derive(Default)]
pub struct Gtfs {
    pub read_duration: i64,
    pub calendar: HashMap<String, Calendar>,
    pub calendar_dates: HashMap<String, Vec<CalendarDate>>,
    pub stops: HashMap<String, Arc<Stop>>,
    pub routes: HashMap<String, Route>,
    pub trips: HashMap<String, Trip>,
    pub agencies: Vec<Agency>,
    pub shapes: HashMap<String, Vec<Shape>>,
    pub fare_attributes: HashMap<String, FareAttribute>,
    pub feed_info: Vec<FeedInfo>,
}

impl RawGtfs {
    pub fn print_stats(&self) {
        println!("GTFS data:");
        println!("  Read in {} ms", self.read_duration);
        println!("  Stops: {}", self.stops.len());
        println!("  Routes: {}", self.routes.len());
        println!("  Trips: {}", self.trips.len());
        println!("  Agencies: {}", self.agencies.len());
        println!("  Shapes: {}", self.shapes.len());
        println!("  Fare attributes: {}", self.fare_attributes.len());
        println!("  Feed info: {}", self.feed_info.len());
    }

    pub fn new(path: &str) -> Result<Self, Error> {
        let now = Utc::now();
        let p = Path::new(path);
        let trips_file = File::open(p.join("trips.txt"))?;
        let calendar_file = File::open(p.join("calendar.txt"))?;
        let stops_file = File::open(p.join("stops.txt"))?;
        let calendar_dates_file = File::open(p.join("calendar_dates.txt"))?;
        let routes_file = File::open(p.join("routes.txt"))?;
        let stop_times_file = File::open(p.join("stop_times.txt"))?;
        let agencies_file = File::open(p.join("agency.txt"))?;
        let shapes_file = File::open(p.join("shapes.txt")).ok();
        let fare_attributes_file = File::open(p.join("fare_attributes.txt")).ok();
        let feed_info_file = File::open(p.join("feed_info.txt")).ok();

        let mut gtfs = Self::default();

        gtfs.trips = read_objs(trips_file)?;
        gtfs.calendar = read_objs(calendar_file)?;
        gtfs.calendar_dates = read_objs(calendar_dates_file)?;
        gtfs.stops = read_objs(stops_file)?;
        gtfs.routes = read_objs(routes_file)?;
        gtfs.stop_times = read_objs(stop_times_file)?;
        gtfs.agencies = read_objs(agencies_file)?;
        if let Some(s_file) = shapes_file {
            gtfs.shapes = read_objs(s_file)?;
        }
        if let Some(f_a_file) = fare_attributes_file {
            gtfs.fare_attributes = read_objs(f_a_file)?;
        }
        if let Some(f_i_file) = feed_info_file {
            gtfs.feed_info = read_objs(f_i_file)?;
        }

        gtfs.read_duration = Utc::now().signed_duration_since(now).num_milliseconds();
        Ok(gtfs)
    }

    pub fn from_zip(file: &str) -> Result<Self, Error> {
        let reader = File::open(file)?;
        Self::from_reader(reader)
    }

    #[cfg(feature = "read-url")]
    pub fn from_url(url: &str) -> Result<Self, Error> {
        let mut res = reqwest::get(url)?;
        let mut body = Vec::new();
        res.read_to_end(&mut body)?;
        let cursor = std::io::Cursor::new(body);
        Self::from_reader(cursor)
    }

    pub fn from_reader<T: std::io::Read + std::io::Seek>(reader: T) -> Result<Self, Error> {
        let now = Utc::now();
        let mut archive = zip::ZipArchive::new(reader)?;
        let mut gtfs = Self::default();
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            if file.name().ends_with("calendar.txt") {
                gtfs.calendar = read_objs(file)
                    .with_context(|e| format!("Error reading calendar.txt : {}", e))?;
            } else if file.name().ends_with("stops.txt") {
                gtfs.stops =
                    read_objs(file).with_context(|e| format!("Error reading stops.txt : {}", e))?;
            } else if file.name().ends_with("calendar_dates.txt") {
                gtfs.calendar_dates = read_objs(file)
                    .with_context(|e| format!("Error reading calendar_dates.txt : {}", e))?;
            } else if file.name().ends_with("routes.txt") {
                gtfs.routes = read_objs(file)
                    .with_context(|e| format!("Error reading routes.txt : {}", e))?;
            } else if file.name().ends_with("trips.txt") {
                gtfs.trips =
                    read_objs(file).with_context(|e| format!("Error reading trips.txt : {}", e))?;
            } else if file.name().ends_with("stop_times.txt") {
                gtfs.stop_times = read_objs(file)
                    .with_context(|e| format!("Error reading stop_times.txt : {}", e))?;
            } else if file.name().ends_with("agency.txt") {
                gtfs.agencies = read_objs(file)
                    .with_context(|e| format!("Error reading agency.txt : {}", e))?;
            } else if file.name().ends_with("shapes.txt") {
                gtfs.shapes = read_objs(file)
                    .with_context(|e| format!("Error reading shapes.txt : {}", e))?;
            } else if file.name().ends_with("fare_attributes.txt") {
                gtfs.fare_attributes = read_objs(file)
                    .with_context(|e| format!("Error reading fare_attributes.txt : {}", e))?;
            } else if file.name().ends_with("feed_info.txt") {
                gtfs.feed_info = read_objs(file)
                    .with_context(|e| format!("Error reading feed_info.txt : {}", e))?;
            }
        }
        gtfs.read_duration = Utc::now().signed_duration_since(now).num_milliseconds();
        Ok(gtfs)
    }
}

fn to_map<O: Id>(elements: impl IntoIterator<Item = O>) -> HashMap<String, O> {
    elements
        .into_iter()
        .map(|e| (e.id().to_owned(), e))
        .collect()
}

fn to_stop_map(stops: Vec<Stop>) -> HashMap<String, Arc<Stop>> {
    stops
        .into_iter()
        .map(|s| (s.id.clone(), Arc::new(s)))
        .collect()
}

fn to_shape_map(shapes: Vec<Shape>) -> HashMap<String, Vec<Shape>> {
    let mut res = HashMap::default();
    for s in shapes {
        let shape = res.entry(s.id.to_owned()).or_insert_with(Vec::new);
        shape.push(s);
    }
    res
}

fn to_calendar_dates(cd: Vec<CalendarDate>) -> HashMap<String, Vec<CalendarDate>> {
    let mut res = HashMap::default();
    for c in cd {
        let cal = res.entry(c.service_id.to_owned()).or_insert_with(Vec::new);
        cal.push(c);
    }
    res
}

fn create_trips(
    raw_trips: Vec<RawTrip>,
    raw_stop_times: Vec<RawStopTime>,
    stops: &HashMap<String, Arc<Stop>>,
) -> Result<HashMap<String, Trip>, Error> {
    let mut trips = to_map(raw_trips.into_iter().map(|rt| Trip {
        id: rt.id,
        service_id: rt.service_id,
        route_id: rt.route_id,
        stop_times: vec![],
    }));
    for s in raw_stop_times {
        let trip = &mut trips.get_mut(&s.trip_id).ok_or(ReferenceError {
            id: s.trip_id.to_string(),
        })?;
        let stop = stops.get(&s.stop_id).ok_or(ReferenceError {
            id: s.stop_id.to_string(),
        })?;
        trip.stop_times.push(StopTime::from(&s, Arc::clone(&stop)));
    }

    for trip in &mut trips.values_mut() {
        trip.stop_times
            .sort_by(|a, b| a.stop_sequence.cmp(&b.stop_sequence));
    }
    Ok(trips)
}

impl Gtfs {
    pub fn try_from(raw: RawGtfs) -> Result<Gtfs, Error> {
        let stops = to_stop_map(raw.stops);
        let trips = create_trips(raw.trips, raw.stop_times, &stops)?;

        Ok(Gtfs {
            stops,
            routes: to_map(raw.routes),
            trips,
            agencies: raw.agencies,
            shapes: to_shape_map(raw.shapes),
            fare_attributes: to_map(raw.fare_attributes),
            feed_info: raw.feed_info,
            calendar: to_map(raw.calendar),
            calendar_dates: to_calendar_dates(raw.calendar_dates),
            read_duration: raw.read_duration,
        })
    }
}

impl Gtfs {
    pub fn print_stats(&self) {
        println!("GTFS data:");
        println!("  Read in {} ms", self.read_duration);
        println!("  Stops: {}", self.stops.len());
        println!("  Routes: {}", self.routes.len());
        println!("  Trips: {}", self.trips.len());
        println!("  Agencies: {}", self.agencies.len());
        println!("  Shapes: {}", self.shapes.len());
        println!("  Fare attributes: {}", self.fare_attributes.len());
        println!("  Feed info: {}", self.feed_info.len());
    }

    pub fn new(path: &str) -> Result<Gtfs, Error> {
        RawGtfs::new(path).and_then(Gtfs::try_from)
    }

    pub fn from_zip(file: &str) -> Result<Gtfs, Error> {
        RawGtfs::from_zip(file).and_then(Gtfs::try_from)
    }

    #[cfg(feature = "read-url")]
    pub fn from_url(url: &str) -> Result<Gtfs, Error> {
        RawGtfs::from_url(url).and_then(Gtfs::try_from)
    }

    pub fn from_reader<T: std::io::Read + std::io::Seek>(reader: T) -> Result<Gtfs, Error> {
        RawGtfs::from_reader(reader).and_then(Gtfs::try_from)
    }

    pub fn trip_days(&self, service_id: &str, start_date: NaiveDate) -> Vec<u16> {
        let mut result = Vec::new();

        // Handle services given by specific days and exceptions
        let mut removed_days = HashSet::new();
        for extra_day in self
            .calendar_dates
            .get(service_id)
            .iter()
            .flat_map(|e| e.iter())
        {
            let offset = extra_day.date.signed_duration_since(start_date).num_days();
            if offset >= 0 {
                if extra_day.exception_type == 1 {
                    result.push(offset as u16);
                } else if extra_day.exception_type == 2 {
                    removed_days.insert(offset);
                }
            }
        }

        if let Some(calendar) = self.calendar.get(service_id) {
            let total_days = calendar
                .end_date
                .signed_duration_since(start_date)
                .num_days();
            for days_offset in 0..=total_days {
                let current_date = start_date + Duration::days(days_offset);

                if calendar.start_date <= current_date
                    && calendar.end_date >= current_date
                    && calendar.valid_weekday(current_date)
                    && !removed_days.contains(&days_offset)
                {
                    result.push(days_offset as u16);
                }
            }
        }

        result
    }

    pub fn get_stop<'a>(&'a self, id: &str) -> Result<&'a Stop, ReferenceError> {
        match self.stops.get(id) {
            Some(stop) => Ok(stop),
            None => Err(ReferenceError { id: id.to_owned() }),
        }
    }

    pub fn get_trip<'a>(&'a self, id: &str) -> Result<&'a Trip, ReferenceError> {
        match self.trips.get(id) {
            Some(trip) => Ok(trip),
            None => Err(ReferenceError { id: id.to_owned() }),
        }
    }

    pub fn get_route<'a>(&'a self, id: &str) -> Result<&'a Route, ReferenceError> {
        match self.routes.get(id) {
            Some(route) => Ok(route),
            None => Err(ReferenceError { id: id.to_owned() }),
        }
    }

    pub fn get_calendar<'a>(&'a self, id: &str) -> Result<&'a Calendar, ReferenceError> {
        match self.calendar.get(id) {
            Some(calendar) => Ok(calendar),
            None => Err(ReferenceError { id: id.to_owned() }),
        }
    }

    pub fn get_calendar_date<'a>(
        &'a self,
        id: &str,
    ) -> Result<&'a Vec<CalendarDate>, ReferenceError> {
        match self.calendar_dates.get(id) {
            Some(calendar_dates) => Ok(calendar_dates),
            None => Err(ReferenceError { id: id.to_owned() }),
        }
    }

    pub fn get_shape<'a>(&'a self, id: &str) -> Result<&'a Vec<Shape>, ReferenceError> {
        match self.shapes.get(id) {
            Some(shape) => Ok(shape),
            None => Err(ReferenceError { id: id.to_owned() }),
        }
    }

    pub fn get_fare_attributes<'a>(
        &'a self,
        id: &str,
    ) -> Result<&'a FareAttribute, ReferenceError> {
        self.fare_attributes
            .get(id)
            .ok_or_else(|| ReferenceError { id: id.to_owned() })
    }
}

fn deserialize_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match &*s {
        "0" => Ok(false),
        "1" => Ok(true),
        &_ => Err(serde::de::Error::custom(format!(
            "Invalid value `{}`, expected 0 or 1",
            s
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_calendar() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        assert_eq!(1, gtfs.calendar.len());
        assert!(!gtfs.calendar["service1"].monday);
        assert!(gtfs.calendar["service1"].saturday);
    }

    #[test]
    fn read_calendar_dates() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        assert_eq!(2, gtfs.calendar_dates.len());
        assert_eq!(2, gtfs.calendar_dates["service1"].len());
        assert_eq!(2, gtfs.calendar_dates["service1"][0].exception_type);
        assert_eq!(1, gtfs.calendar_dates["service2"][0].exception_type);
    }

    #[test]
    fn read_stop() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        assert_eq!(5, gtfs.stops.len());
        assert_eq!(
            LocationType::StopArea,
            gtfs.get_stop("stop1").unwrap().location_type
        );
        assert_eq!(
            LocationType::StopPoint,
            gtfs.get_stop("stop2").unwrap().location_type
        );
        assert_eq!(
            Some("1".to_owned()),
            gtfs.get_stop("stop3").unwrap().parent_station
        );
    }

    #[test]
    fn read_routes() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        assert_eq!(2, gtfs.routes.len());
        assert_eq!(RouteType::Bus, gtfs.get_route("1").unwrap().route_type);
        assert_eq!(
            RouteType::Other(42),
            gtfs.get_route("invalid_type").unwrap().route_type
        );
    }

    #[test]
    fn read_trips() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        assert_eq!(1, gtfs.trips.len());
    }

    #[test]
    fn read_stop_times() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        let stop_times = &gtfs.trips.get("trip1").unwrap().stop_times;
        assert_eq!(2, stop_times.len());
        assert_eq!(
            PickupDropOffType::Regular,
            stop_times[0].pickup_type.unwrap()
        );
        assert_eq!(
            PickupDropOffType::NotAvailable,
            stop_times[0].drop_off_type.unwrap()
        );
        assert_eq!(
            PickupDropOffType::ArrangeByPhone,
            stop_times[1].pickup_type.unwrap()
        );
        assert_eq!(None, stop_times[1].drop_off_type);
    }

    #[test]
    fn read_agencies() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        let agencies = &gtfs.agencies;
        assert_eq!("BIBUS", agencies[0].name);
        assert_eq!("http://www.bibus.fr", agencies[0].url);
        assert_eq!("Europe/Paris", agencies[0].timezone);
    }

    #[test]
    fn read_shapes() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        let shapes = &gtfs.shapes;
        assert_eq!(37.61956, shapes["A_shp"][0].latitude);
        assert_eq!(-122.48161, shapes["A_shp"][0].longitude);
    }

    #[test]
    fn read_fare_attributes() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        assert_eq!(1, gtfs.fare_attributes.len());
        assert_eq!("1.50", gtfs.get_fare_attributes("50").unwrap().price);
        assert_eq!("EUR", gtfs.get_fare_attributes("50").unwrap().currency);
        assert_eq!(
            PaymentMethod::Aboard,
            gtfs.get_fare_attributes("50").unwrap().payment_method
        );
        assert_eq!(
            Transfers::Unlimited,
            gtfs.get_fare_attributes("50").unwrap().transfers
        );
        assert_eq!(
            Some("1".to_string()),
            gtfs.get_fare_attributes("50").unwrap().agency_id
        );
        assert_eq!(
            Some(3600),
            gtfs.get_fare_attributes("50").unwrap().transfer_duration
        );
    }

    #[test]
    fn read_feed_info() {
        let gtfs = Gtfs::new("fixtures").expect("impossible to read gtfs");
        let feed = &gtfs.feed_info;
        assert_eq!(1, feed.len());
        assert_eq!("SNCF", feed[0].name);
        assert_eq!("http://www.sncf.com", feed[0].url);
        assert_eq!("fr", feed[0].lang);
        assert_eq!(Some(NaiveDate::from_ymd(2018, 07, 09)), feed[0].start_date);
        assert_eq!(Some(NaiveDate::from_ymd(2018, 09, 27)), feed[0].end_date);
        assert_eq!(Some("0.3".to_string()), feed[0].version);
    }

    #[test]
    fn trip_days() {
        let gtfs = Gtfs::new("fixtures/").unwrap();
        let days = gtfs.trip_days(&"service1".to_owned(), NaiveDate::from_ymd(2017, 1, 1));
        assert_eq!(vec![6, 7, 13, 14], days);

        let days2 = gtfs.trip_days(&"service2".to_owned(), NaiveDate::from_ymd(2017, 1, 1));
        assert_eq!(vec![0], days2);
    }

    #[test]
    fn read_from_gtfs() {
        let gtfs = Gtfs::from_zip("fixtures/gtfs.zip").unwrap();
        assert_eq!(1, gtfs.calendar.len());
        assert_eq!(2, gtfs.calendar_dates.len());
        assert_eq!(5, gtfs.stops.len());
        assert_eq!(1, gtfs.routes.len());
        assert_eq!(1, gtfs.trips.len());
        assert_eq!(1, gtfs.shapes.len());
        assert_eq!(1, gtfs.fare_attributes.len());
        assert_eq!(1, gtfs.feed_info.len());
        assert_eq!(2, gtfs.get_trip("trip1").unwrap().stop_times.len());

        assert!(gtfs.get_calendar("service1").is_ok());
        assert!(gtfs.get_calendar_date("service1").is_ok());
        assert!(gtfs.get_stop("stop1").is_ok());
        assert!(gtfs.get_route("1").is_ok());
        assert!(gtfs.get_trip("trip1").is_ok());
        assert!(gtfs.get_fare_attributes("50").is_ok());

        assert_eq!("Utopia", gtfs.get_stop("Utopia").unwrap_err().id);
    }

    #[test]
    fn read_from_subdirectory() {
        let gtfs = Gtfs::from_zip("fixtures/subdirectory.zip").unwrap();
        assert_eq!(1, gtfs.calendar.len());
        assert_eq!(2, gtfs.calendar_dates.len());
        assert_eq!(5, gtfs.stops.len());
        assert_eq!(1, gtfs.routes.len());
        assert_eq!(1, gtfs.trips.len());
        assert_eq!(1, gtfs.shapes.len());
        assert_eq!(1, gtfs.fare_attributes.len());
        assert_eq!(2, gtfs.get_trip("trip1").unwrap().stop_times.len());
    }

    #[test]
    fn display() {
        assert_eq!(
            "Sorano".to_owned(),
            format!(
                "{}",
                Stop {
                    name: "Sorano".to_owned(),
                    ..Stop::default()
                }
            )
        );

        assert_eq!(
            "Long route name".to_owned(),
            format!(
                "{}",
                Route {
                    long_name: "Long route name".to_owned(),
                    ..Route::default()
                }
            )
        );

        assert_eq!(
            "Short route name".to_owned(),
            format!(
                "{}",
                Route {
                    short_name: "Short route name".to_owned(),
                    ..Route::default()
                }
            )
        );
    }
}
